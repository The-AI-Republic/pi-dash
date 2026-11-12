#![forbid(unsafe_code)]

//! GitHub pull-request links for work items (`utils/github_pr_links.py:1-174`).
//!
//! Port of the shared attach/detach service used by both API surfaces:
//!
//! * [`attach_pull_request`] (`:119-174`) — parse, issue check, dedupe,
//!   best-effort snapshot, create + mirror, IntegrityError race.
//! * [`detach_pull_request_link`] (`:112-116`) — delete the link and its
//!   same-issue mirror.
//! * `ensure_code_review_link` (`:76-109`) — the `GitCodeReviewLink`
//!   mirror (private, as in Python).
//! * [`best_effort_snapshot`] (`:44-59`) — installation lookup +
//!   `GithubClient` fetch, `{}` on any failure.
//! * [`parse_pr_url`] (`github_client.py:259-275` via `:127`) and
//!   [`pr_snapshot_from_payload`] (`github_client.py:287-300`).
//!
//! # Layering: a seam, not direct calls
//!
//! This crate carries no database handle, so the verbs are generic over
//! the [`PrLinksStore`] seam (the `PubsubStore` precedent): one method
//! per underlying effect, each naming the SQL text the pool
//! implementation executes verbatim. The failure policy (which errors
//! propagate, which become log lines) lives here, exactly as in Python.
//!
//! # Failure policy (verbatim from the `try`/`except` sites)
//!
//! | site | behaviour |
//! |---|---|
//! | `best_effort_snapshot` (`:57-59`) | ANY failure (no installation,
//!   connect/token/network/404) → empty snapshot + one warning line |
//! | attach create (`:165-174`) | unique-violation → re-read and resolve;
//!   any other store failure propagates |
//! | attach/detach otherwise | typed errors propagate (no `try` in Python) |
//!
//! This crate has no logger, so swallowed failures come back as warning
//! lines (the `SendOutcome::warnings` precedent); the caller logs each
//! line. The snapshot warning is the exception message verbatim (what
//! Python's `log_exception` logs via `logger.exception`, minus the
//! traceback, which Rust cannot produce).
//!
//! # Error mapping
//!
//! The service raises typed errors; the views map them (fixture
//! `views_map`): [`InvalidPullRequestURL`] → 400
//! `{'error': 'A valid github.com pull request URL is required.'}`,
//! [`IssueNotFound`] → 404 `{'error': 'Work item not found.'}`,
//! [`PullRequestAlreadyLinked`] → 409 `{'error': 'This pull request is
//! already linked to issue <id>.'}`. The exact wire bytes are owned by
//! the handler ports (D-18 / PIDASHCONV-653), which pin them from their
//! own fixtures.
//!
//! # Reuse notes
//!
//! * [`GithubClient`](crate::integrations::adapters_github::GithubClient),
//!   [`ClientAuth::Installation`](crate::integrations::adapters_github::ClientAuth::Installation),
//!   [`parse_dt`](crate::integrations::adapters_github::parse_dt) and
//!   [`GITHUB_HOST`](crate::integrations::adapters_github::GITHUB_HOST)
//!   are reused from `integrations::adapters_github`, never re-ported.
//! * `parse_github_pull_request_url` and `pr_snapshot` there are private,
//!   so this module ports them locally ([`parse_pr_url`],
//!   [`pr_snapshot_from_payload`]) — the `assistant::tools_github` /
//!   `api::app_integrations::handlers_github_app` precedent. The parse
//!   keeps the URL case (lowercasing happens in [`attach_pull_request`],
//!   per `github_pr_links.py:133`); the merged ports lowercase inside.
//! * Instance `delete()` fans out
//!   `soft_delete_related_objects("db", model, pk, using=None)`
//!   (`db/mixins.py:72-78`); the detach outcome carries those enqueues
//!   as data ([`TaskEnqueue`]) for the executing layer. The task string
//!   is pinned by value against
//!   `pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK` (this crate
//!   cannot import `pidash-jobs`).
//!
//! # Ported quirks (translate, don't redesign)
//!
//! * The attach race re-read (`.first()`, no lock) keeps its TOCTOU gap.
//! * Non-string `title` in a PR payload renders `""` (Python would crash
//!   slicing it; unreachable via the real API — the adapters precedent).
//! * Absurd digit runs in the PR number 500 at parse time here (Python
//!   would 500 later at INSERT with a `DataError`).
//!
//! Fixture: `rust-api/fixtures/app_issues/queries/FX-ISS-13.move.json`
//! (`pr_links_goldens`). Every section is replayed by the `#[cfg(test)]`
//! suite below.

use crate::integrations::adapters_github::{parse_dt, ClientAuth, GithubClient, GITHUB_HOST};
use serde_json::{Map, Value};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// The supplied string is not a valid github.com pull request URL
/// (`InvalidPullRequestURL`, `:28-29`). Carries no message (Python raises
/// it bare); the view renders the 400 body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidPullRequestURL;

/// The work item does not exist in the given project/workspace
/// (`IssueNotFound`, `:32-33`). Carries no message; the view renders the
/// 404 body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IssueNotFound;

/// The PR is already linked to a different work item (one PR → one issue).
/// (`PullRequestAlreadyLinked`, `:36-41`: `__init__(self, issue_id)`.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PullRequestAlreadyLinked {
    /// `existing.issue_id` — the issue the PR already belongs to.
    pub issue_id: Uuid,
}

/// A storage failure from the [`PrLinksStore`] seam (connection, mapping,
/// unexpected constraint). The pool implementation maps these to a 500;
/// the unique-violation race is [`CreatePrLinkOutcome::Conflict`], never
/// this error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(pub String);

/// Outcome of the link INSERT ([`PrLinksStore::create_pr_link`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreatePrLinkOutcome {
    /// The row was inserted (boxed: the enum would otherwise carry a
    /// 200-byte variant next to a fieldless one).
    Created(Box<PrLinkRow>),
    /// A concurrent attach won the partial-unique race
    /// (`github_pr_link_unique_per_pr_when_active`): re-read and resolve
    /// (`:165-174`), never a 500.
    Conflict,
}

// ---------------------------------------------------------------------------
// SQL text (Django shape; the pool implementation executes verbatim)
// ---------------------------------------------------------------------------

/// Installation lookup for [`best_effort_snapshot`] (`:48-51`):
/// `filter(workspace_integration__workspace__slug, account_login__iexact)`
/// `.first()`. Soft-delete scope on the installation table only (FK joins
/// carry no manager scope); `iexact` is `LOWER()` on both sides;
/// `-created_at` ordering. `$1` workspace slug, `$2` owner login.
pub const INSTALLATION_LOOKUP_SQL: &str = "SELECT \"github_app_installations\".\"id\", \"github_app_installations\".\"created_at\", \"github_app_installations\".\"updated_at\", \"github_app_installations\".\"created_by_id\", \"github_app_installations\".\"updated_by_id\", \"github_app_installations\".\"deleted_at\", \"github_app_installations\".\"workspace_integration_id\", \"github_app_installations\".\"installation_id\", \"github_app_installations\".\"account_login\", \"github_app_installations\".\"account_type\", \"github_app_installations\".\"repository_selection\", \"github_app_installations\".\"repository_count\", \"github_app_installations\".\"permissions\", \"github_app_installations\".\"events\", \"github_app_installations\".\"installed_at\", \"github_app_installations\".\"suspended_at\", \"github_app_installations\".\"verified_at\", \"github_app_installations\".\"last_checked_at\", \"github_app_installations\".\"last_check_error\" FROM \"github_app_installations\" INNER JOIN \"workspace_integrations\" ON (\"github_app_installations\".\"workspace_integration_id\" = \"workspace_integrations\".\"id\") INNER JOIN \"workspaces\" ON (\"workspace_integrations\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"github_app_installations\".\"deleted_at\" IS NULL AND \"workspaces\".\"slug\" = $1 AND LOWER(\"github_app_installations\".\"account_login\") = LOWER($2)) ORDER BY \"github_app_installations\".\"created_at\" DESC LIMIT 1";

/// Issue-membership probe for [`attach_pull_request`] (`:138-139`):
/// `Issue.objects.filter(id, project_id, workspace__slug).exists()`.
/// `Issue.objects` is the plain inherited soft-delete scope (triage /
/// archived / draft rows still count — NOT the narrower
/// `issue_objects`). `$1` issue id, `$2` project id, `$3` slug.
pub const ISSUE_EXISTS_SQL: &str = "SELECT 1 AS \"a\" FROM \"issues\" INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"id\" = $1 AND \"issues\".\"project_id\" = $2 AND \"workspaces\".\"slug\" = $3) LIMIT 1";

/// Existing-link lookup (`:141`, `:168`): full row, soft-delete scope,
/// `-created_at` ordering. `$1` owner, `$2` name, `$3` PR number.
pub const PR_LINK_LOOKUP_SQL: &str = "SELECT \"github_pull_request_links\".\"id\", \"github_pull_request_links\".\"created_at\", \"github_pull_request_links\".\"updated_at\", \"github_pull_request_links\".\"created_by_id\", \"github_pull_request_links\".\"updated_by_id\", \"github_pull_request_links\".\"deleted_at\", \"github_pull_request_links\".\"project_id\", \"github_pull_request_links\".\"workspace_id\", \"github_pull_request_links\".\"issue_id\", \"github_pull_request_links\".\"repo_owner\", \"github_pull_request_links\".\"repo_name\", \"github_pull_request_links\".\"pr_number\", \"github_pull_request_links\".\"url\", \"github_pull_request_links\".\"title\", \"github_pull_request_links\".\"state\", \"github_pull_request_links\".\"merged\", \"github_pull_request_links\".\"draft\", \"github_pull_request_links\".\"pr_updated_at\" FROM \"github_pull_request_links\" WHERE (\"github_pull_request_links\".\"deleted_at\" IS NULL AND \"github_pull_request_links\".\"repo_owner\" = $1 AND \"github_pull_request_links\".\"repo_name\" = $2 AND \"github_pull_request_links\".\"pr_number\" = $3) ORDER BY \"github_pull_request_links\".\"created_at\" DESC LIMIT 1";

/// Mirror lookup (`_matching_code_review_link`, `:62-73`): full row,
/// soft-delete scope, `-created_at` ordering. `$1` owner (namespace),
/// `$2` repo name, `$3` external iid (the canonical number string).
pub const REVIEW_LINK_LOOKUP_SQL: &str = "SELECT \"git_code_review_links\".\"id\", \"git_code_review_links\".\"created_at\", \"git_code_review_links\".\"updated_at\", \"git_code_review_links\".\"created_by_id\", \"git_code_review_links\".\"updated_by_id\", \"git_code_review_links\".\"deleted_at\", \"git_code_review_links\".\"project_id\", \"git_code_review_links\".\"workspace_id\", \"git_code_review_links\".\"issue_id\", \"git_code_review_links\".\"provider\", \"git_code_review_links\".\"host_url\", \"git_code_review_links\".\"namespace\", \"git_code_review_links\".\"repo_name\", \"git_code_review_links\".\"repo_external_id\", \"git_code_review_links\".\"external_id\", \"git_code_review_links\".\"external_iid\", \"git_code_review_links\".\"url\", \"git_code_review_links\".\"title\", \"git_code_review_links\".\"state\", \"git_code_review_links\".\"merged\", \"git_code_review_links\".\"draft\", \"git_code_review_links\".\"remote_updated_at\", \"git_code_review_links\".\"metadata\" FROM \"git_code_review_links\" WHERE (\"git_code_review_links\".\"deleted_at\" IS NULL AND \"git_code_review_links\".\"provider\" = 'github' AND \"git_code_review_links\".\"host_url\" = 'https://github.com' AND \"git_code_review_links\".\"namespace\" = $1 AND \"git_code_review_links\".\"repo_name\" = $2 AND \"git_code_review_links\".\"external_iid\" = $3) ORDER BY \"git_code_review_links\".\"created_at\" DESC LIMIT 1";

/// Project workspace resolution for creates: `ProjectBaseModel.save`
/// overwrites `workspace` with `project.workspace` on every save, so the
/// pool implementation resolves `$1` (project id) through this lookup and
/// binds the result as `workspace_id` on the INSERTs below. (Python pays
/// the same lazy FK fetch via `self.project.workspace`.)
pub const PROJECT_WORKSPACE_LOOKUP_SQL: &str =
    "SELECT \"projects\".\"workspace_id\" FROM \"projects\" WHERE \"projects\".\"id\" = $1 LIMIT 1";

/// Link INSERT (`:154-161`): every concrete column in field order.
/// `$1` id (uuid4, generated here), `$2`/`$3` created/updated (`now`),
/// `$4` created_by (actor, NULL when anonymous), `$5` project,
/// `$6` workspace (resolved per [`PROJECT_WORKSPACE_LOOKUP_SQL`]),
/// `$7` issue, `$8`-`$10` owner/name/number, `$11` url, `$12`-`$15`
/// snapshot, `$16` pr_updated_at (NULL when unknown). `updated_by_id`
/// and `deleted_at` insert NULL.
pub const PR_LINK_INSERT_SQL: &str = "INSERT INTO \"github_pull_request_links\" (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"project_id\", \"workspace_id\", \"issue_id\", \"repo_owner\", \"repo_name\", \"pr_number\", \"url\", \"title\", \"state\", \"merged\", \"draft\", \"pr_updated_at\") VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)";

/// Mirror INSERT (`:100-109`): field order; `repo_external_id` and
/// `external_id` insert `""`. `$1` id, `$2`/`$3` created/updated,
/// `$4` created_by, `$5` project, `$6` workspace, `$7` issue, `$8` url,
/// `$9` title, `$10` state, `$11` merged, `$12` draft, `$13`
/// remote_updated_at, `$14` metadata, `$15`-`$17` namespace/repo/iid.
pub const REVIEW_LINK_INSERT_SQL: &str = "INSERT INTO \"git_code_review_links\" (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \"deleted_at\", \"project_id\", \"workspace_id\", \"issue_id\", \"provider\", \"host_url\", \"namespace\", \"repo_name\", \"repo_external_id\", \"external_id\", \"external_iid\", \"url\", \"title\", \"state\", \"merged\", \"draft\", \"remote_updated_at\", \"metadata\") VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, 'github', 'https://github.com', $15, $16, '', '', $17, $8, $9, $10, $11, $12, $13, $14)";

/// Mirror UPDATE (`:96-98`, full `save()` collapsed to its observable
/// writes — unchanged columns rewrite identical values): `$1`
/// updated_at, `$2` updated_by, `$3` project, `$4` workspace, `$5`
/// issue, `$6` url, `$7` title, `$8` state, `$9` merged, `$10` draft,
/// `$11` remote_updated_at, `$12` metadata, `$13` id.
pub const REVIEW_LINK_UPDATE_SQL: &str = "UPDATE \"git_code_review_links\" SET \"updated_at\" = $1, \"updated_by_id\" = $2, \"project_id\" = $3, \"workspace_id\" = $4, \"issue_id\" = $5, \"url\" = $6, \"title\" = $7, \"state\" = $8, \"merged\" = $9, \"draft\" = $10, \"remote_updated_at\" = $11, \"metadata\" = $12 WHERE \"git_code_review_links\".\"id\" = $13";

/// Instance `delete()` (`db/mixins.py:72-76`, full save collapsed):
/// `$1` deleted/updated stamp, `$2` updated_by, `$3` id. Each delete
/// also enqueues [`SOFT_DELETE_TASK`] (see [`TaskEnqueue`]).
pub const PR_LINK_SOFT_DELETE_SQL: &str = "UPDATE \"github_pull_request_links\" SET \"deleted_at\" = $1, \"updated_at\" = $1, \"updated_by_id\" = $2 WHERE \"github_pull_request_links\".\"id\" = $3";

/// Mirror instance `delete()`: same shape as [`PR_LINK_SOFT_DELETE_SQL`].
pub const REVIEW_LINK_SOFT_DELETE_SQL: &str = "UPDATE \"git_code_review_links\" SET \"deleted_at\" = $1, \"updated_at\" = $1, \"updated_by_id\" = $2 WHERE \"git_code_review_links\".\"id\" = $3";

/// `soft_delete_related_objects` (`bgtasks/deletion_task.py:18`): full
/// Celery name, plain `@shared_task`. Pinned by value against
/// `pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK`.
pub const SOFT_DELETE_TASK: &str = "pi_dash.bgtasks.deletion_task.soft_delete_related_objects";

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// One `GithubAppInstallation` row as [`best_effort_snapshot`] consumes
/// it: only `installation_id` is read (the client auth); the lookup SQL
/// still projects the full row, mirroring `.first()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallationRow {
    pub id: Uuid,
    pub installation_id: i64,
}

/// One `GithubPullRequestLink` row: every column the service reads
/// (mirror fields + identity for the dedupe arms).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrLinkRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub issue_id: Uuid,
    pub repo_owner: String,
    pub repo_name: String,
    pub pr_number: i64,
    pub url: String,
    pub title: String,
    /// `open` / `closed` (`State` choices).
    pub state: String,
    pub merged: bool,
    pub draft: bool,
    /// ISO rendering of `pr_updated_at` (NULL when unknown).
    pub pr_updated_at: Option<String>,
}

/// One `GitCodeReviewLink` row as the mirror logic consumes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewLinkRow {
    pub id: Uuid,
    pub issue_id: Uuid,
    /// Never NULL (`JSONField(default=dict)`).
    pub metadata: Value,
}

/// A new link row for [`PrLinksStore::create_pr_link`]: id generated
/// here (uuid4, as Django does client-side); timestamps and actor bound
/// by the implementation from the call's `now` / `actor_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPrLink {
    pub id: Uuid,
    pub project_id: Uuid,
    pub issue_id: Uuid,
    pub repo_owner: String,
    pub repo_name: String,
    pub pr_number: i64,
    pub url: String,
    pub title: String,
    pub state: String,
    pub merged: bool,
    pub draft: bool,
    pub pr_updated_at: Option<String>,
}

/// A new mirror row for [`PrLinksStore::create_review_link`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewReviewLink {
    pub id: Uuid,
    pub project_id: Uuid,
    pub issue_id: Uuid,
    pub namespace: String,
    pub repo_name: String,
    pub external_iid: String,
    pub url: String,
    pub title: String,
    pub state: String,
    pub merged: bool,
    pub draft: bool,
    pub remote_updated_at: Option<String>,
    pub metadata: Value,
}

/// Mirror overwrite fields for [`PrLinksStore::update_review_link`]
/// (`fields` dict, `:79-92`, in source order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewMirrorFields {
    pub project_id: Uuid,
    pub issue_id: Uuid,
    pub url: String,
    pub title: String,
    pub state: String,
    pub merged: bool,
    pub draft: bool,
    pub remote_updated_at: Option<String>,
    pub metadata: Value,
}

/// One `.delay(...)` enqueue the executing layer publishes (Celery
/// positional args + kwargs, in Python call order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskEnqueue {
    pub task: &'static str,
    pub args: Vec<Value>,
    pub kwargs: Map<String, Value>,
}

/// The instance-`delete()` fan-out (`db/mixins.py:78`):
/// `soft_delete_related_objects.delay(app_label, model_name, pk,
/// using=None)` — positional `using=None` kwarg, exactly as Python's
/// call sends it.
pub fn soft_delete_enqueue(model_name: &str, pk: &Uuid) -> TaskEnqueue {
    let mut kwargs = Map::with_capacity(1);
    kwargs.insert("using".to_owned(), Value::Null);
    TaskEnqueue {
        task: SOFT_DELETE_TASK,
        args: vec![
            Value::String("db".to_owned()),
            Value::String(model_name.to_owned()),
            Value::String(pk.to_string()),
        ],
        kwargs,
    }
}

// ---------------------------------------------------------------------------
// Pure ports: URL parse, snapshot, truthiness
// ---------------------------------------------------------------------------

/// A parsed PR URL: case preserved (the caller lowercases per `:133`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPrUrl {
    pub owner: String,
    pub name: String,
    /// Canonical number (`str(int(n))`: leading zeros stripped, no width
    /// limit — kept a string so absurd digit runs never round-trip
    /// through a machine int here).
    pub number: String,
}

/// `parse_github_pull_request_url` (`github_client.py:259-275`) over
/// `_HTTPS_PR_RE` (`:259-261`):
/// `^https?://github\.com/([^/\s]+)/([^/\s]+?)/pull/(\d+)(?:/[^\s]*)?$`.
///
/// Scheme/host match case-sensitively (no `re.IGNORECASE`); the repo
/// name keeps a `.git` suffix (no suffix group in the regex, unlike the
/// repo-URL pattern); any whitespace-free trailing path is allowed.
/// `\d` is ASCII-only here (the merged ports' rule; Python's Unicode
/// digits in a pasted URL are pathological).
pub fn parse_pr_url(url: &str) -> Option<ParsedPrUrl> {
    if url.is_empty() {
        return None;
    }
    let candidate = url.trim();
    if candidate.is_empty() {
        return None;
    }
    let rest = candidate
        .strip_prefix("https://github.com/")
        .or_else(|| candidate.strip_prefix("http://github.com/"))?;
    let mut segments = rest.split('/');
    let owner = segments.next()?;
    let name = segments.next()?;
    if owner.is_empty() || name.is_empty() {
        return None;
    }
    if segments.next() != Some("pull") {
        return None;
    }
    let number = segments.next()?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if has_whitespace(owner) || has_whitespace(name) {
        return None;
    }
    for tail in segments {
        if has_whitespace(tail) {
            return None;
        }
    }
    // `int(group)` then `str()`: strip leading zeros with no width
    // limit (the `tools_github` precedent).
    let stripped = number.trim_start_matches('0');
    let canonical = if stripped.is_empty() { "0" } else { stripped };
    Some(ParsedPrUrl {
        owner: owner.to_owned(),
        name: name.to_owned(),
        number: canonical.to_owned(),
    })
}

/// Python `str.isspace` coverage for URL segments (`re` `\s` under `str`
/// patterns matches Unicode whitespace).
fn has_whitespace(s: &str) -> bool {
    s.chars().any(char::is_whitespace)
}

/// Python `bool()` over a JSON value (`merged`/`draft` derivation).
fn json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else {
                n.as_f64().is_some_and(|f| f != 0.0)
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Display-only PR snapshot (`pr_snapshot_from_payload`,
/// `github_client.py:287-300`), resolved (defaults applied — the empty
/// `{}` from a failed fetch renders the field defaults at create).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrSnapshot {
    pub title: String,
    pub state: String,
    pub merged: bool,
    pub draft: bool,
    pub pr_updated_at: Option<String>,
}

/// Field defaults applied when the snapshot is empty (`github.py:238-242`).
pub fn default_snapshot() -> PrSnapshot {
    PrSnapshot {
        title: String::new(),
        state: "open".to_owned(),
        merged: false,
        draft: false,
        pr_updated_at: None,
    }
}

/// `pr_snapshot_from_payload` (`github_client.py:287-300`).
///
/// `merged` derives from either `merged` or `merged_at` (webhooks report
/// the latter); `title` is `(title or "")[:500]` — 500 *characters*
/// (Python slicing counts code points); any non-`"closed"` state —
/// including a missing one — becomes `"open"`; `pr_updated_at` parses
/// via the reused [`parse_dt`] (`None` on any failure).
pub fn pr_snapshot_from_payload(pull_request: &Value) -> PrSnapshot {
    let merged = pull_request.get("merged").is_some_and(json_truthy)
        || pull_request.get("merged_at").is_some_and(json_truthy);
    let title: String = pull_request
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(500)
        .collect();
    let state = match pull_request.get("state") {
        Some(Value::String(text)) if text == "closed" => "closed".to_owned(),
        _ => "open".to_owned(),
    };
    PrSnapshot {
        title,
        state,
        merged,
        draft: pull_request.get("draft").is_some_and(json_truthy),
        pr_updated_at: parse_dt(pull_request.get("updated_at").and_then(Value::as_str)),
    }
}

// ---------------------------------------------------------------------------
// Seam
// ---------------------------------------------------------------------------

/// Storage seam for the PR-link verbs.
///
/// Methods mirror the ORM calls in `github_pr_links.py`, one per effect;
/// the pool implementation executes the named SQL text verbatim. Uuids
/// arrive typed. `now` is the frozen clock (`timezone.now()`); `actor_id`
/// is the CRUM request user (`None` = anonymous → NULL audit columns).
///
/// Native `async fn` in trait (stable since 1.75): no `async-trait`
/// dependency enters the lockfile for this seam (the `GitStore`
/// precedent).
#[allow(async_fn_in_trait)]
pub trait PrLinksStore {
    /// Installation lookup ([`INSTALLATION_LOOKUP_SQL`]).
    async fn installation_for_account(
        &self,
        workspace_slug: &str,
        owner: &str,
    ) -> Result<Option<InstallationRow>, StoreError>;

    /// Issue-membership probe ([`ISSUE_EXISTS_SQL`]).
    async fn issue_exists(
        &self,
        issue_id: Uuid,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<bool, StoreError>;

    /// Existing-link lookup ([`PR_LINK_LOOKUP_SQL`]).
    async fn pr_link_by_pr(
        &self,
        owner: &str,
        name: &str,
        number: i64,
    ) -> Result<Option<PrLinkRow>, StoreError>;

    /// Mirror lookup ([`REVIEW_LINK_LOOKUP_SQL`]). `number` is the
    /// canonical number string (`str(int(n))`).
    async fn review_link_by_pr(
        &self,
        owner: &str,
        name: &str,
        number: &str,
    ) -> Result<Option<ReviewLinkRow>, StoreError>;

    /// Link INSERT ([`PR_LINK_INSERT_SQL`]). Maps the
    /// `github_pr_link_unique_per_pr_when_active` violation to
    /// [`CreatePrLinkOutcome::Conflict`]; any other failure is a
    /// [`StoreError`] (re-raised, `:169-170`).
    async fn create_pr_link(
        &self,
        new: &NewPrLink,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<CreatePrLinkOutcome, StoreError>;

    /// Mirror INSERT ([`REVIEW_LINK_INSERT_SQL`]).
    async fn create_review_link(
        &self,
        new: &NewReviewLink,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<ReviewLinkRow, StoreError>;

    /// Mirror overwrite ([`REVIEW_LINK_UPDATE_SQL`]).
    async fn update_review_link(
        &self,
        id: Uuid,
        fields: &ReviewMirrorFields,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<(), StoreError>;

    /// Link instance `delete()` ([`PR_LINK_SOFT_DELETE_SQL`]).
    async fn delete_pr_link(
        &self,
        id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<(), StoreError>;

    /// Mirror instance `delete()` ([`REVIEW_LINK_SOFT_DELETE_SQL`]).
    async fn delete_review_link(
        &self,
        id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<(), StoreError>;
}

// ---------------------------------------------------------------------------
// Verbs
// ---------------------------------------------------------------------------

/// Best-effort PR snapshot (`best_effort_snapshot`, `:44-59`).
///
/// Fetches the PR via the workspace installation covering `owner`
/// ([`INSTALLATION_LOOKUP_SQL`]), then `GithubClient.for_installation`
/// and `get_pull_request`. Returns the resolved snapshot plus the
/// warning line (`None` when the fetch succeeded): on ANY failure —
/// no installation, connect, token, network, 404 — the snapshot is the
/// field defaults and the warning is the exception message verbatim
/// (what `log_exception` logs).
pub fn best_effort_snapshot<C: GithubClient>(
    installation: Option<&InstallationRow>,
    owner: &str,
    name: &str,
    number: i64,
) -> (PrSnapshot, Option<String>) {
    let Some(row) = installation else {
        return (default_snapshot(), None);
    };
    let fetched = (|| -> Result<Value, crate::integrations::adapters_github::GithubError> {
        let client = C::connect(&ClientAuth::Installation(row.installation_id))?;
        client.get_pull_request(owner, name, number)
    })();
    match fetched {
        Ok(payload) => (pr_snapshot_from_payload(&payload), None),
        Err(error) => (default_snapshot(), Some(error.message().to_owned())),
    }
}

/// Mirror state for a link (`:78`): merged wins, else closed, else open.
pub fn mirror_state(link: &PrLinkRow) -> String {
    if link.merged {
        "merged".to_owned()
    } else if link.state == "closed" {
        "closed".to_owned()
    } else {
        "open".to_owned()
    }
}

/// Merged mirror metadata (`:88-91`): the existing metadata (or `{}`)
/// plus the legacy link id. `metadata` is never NULL
/// (`JSONField(default=dict)`); a non-object value passes through
/// untouched and the legacy key is still added — matching `{**meta,
/// ...}` for every dict and degrading gracefully otherwise. (Python
/// would crash unpacking a non-dict; unreachable for rows this service
/// writes.)
fn merged_metadata(existing: Option<&Value>, link_id: &Uuid) -> Value {
    let mut map = match existing {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    map.insert(
        "legacy_github_pull_request_link_id".to_owned(),
        Value::String(link_id.to_string()),
    );
    Value::Object(map)
}

/// The mirror overwrite fields for a link (`fields`, `:79-92`).
fn mirror_fields(link: &PrLinkRow, existing: Option<&ReviewLinkRow>) -> ReviewMirrorFields {
    ReviewMirrorFields {
        project_id: link.project_id,
        issue_id: link.issue_id,
        url: link.url.clone(),
        title: link.title.clone(),
        state: mirror_state(link),
        merged: link.merged,
        draft: link.draft,
        remote_updated_at: link.pr_updated_at.clone(),
        metadata: merged_metadata(existing.map(|row| &row.metadata), &link.id),
    }
}

/// Failure modes of [`attach_pull_request`]: the three typed domain
/// errors plus [`StoreError`] (anything the seam reports — connection,
/// mapping, or a non-race constraint — which the handler maps to a 500,
/// as Django's uncaught exception would).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachError {
    InvalidUrl(InvalidPullRequestURL),
    IssueNotFound(IssueNotFound),
    AlreadyLinked(PullRequestAlreadyLinked),
    Store(StoreError),
}

impl From<StoreError> for AttachError {
    fn from(error: StoreError) -> Self {
        AttachError::Store(error)
    }
}

impl From<PullRequestAlreadyLinked> for AttachError {
    fn from(error: PullRequestAlreadyLinked) -> Self {
        AttachError::AlreadyLinked(error)
    }
}

/// `_ensure_code_review_link` (`:76-109`): create the mirror, or
/// overwrite it when it already exists on the same issue; a mirror on
/// another issue raises [`PullRequestAlreadyLinked`].
async fn ensure_code_review_link<S: PrLinksStore>(
    store: &S,
    link: &PrLinkRow,
    now: &chrono::DateTime<chrono::Utc>,
    actor_id: Option<Uuid>,
) -> Result<(), AttachError> {
    let existing = store
        .review_link_by_pr(
            &link.repo_owner,
            &link.repo_name,
            &link.pr_number.to_string(),
        )
        .await?;
    let fields = mirror_fields(link, existing.as_ref());
    match existing {
        Some(row) => {
            if row.issue_id != link.issue_id {
                return Err(PullRequestAlreadyLinked {
                    issue_id: row.issue_id,
                }
                .into());
            }
            store
                .update_review_link(row.id, &fields, now, actor_id)
                .await?;
        }
        None => {
            store
                .create_review_link(
                    &NewReviewLink {
                        id: Uuid::new_v4(),
                        project_id: fields.project_id,
                        issue_id: fields.issue_id,
                        namespace: link.repo_owner.clone(),
                        repo_name: link.repo_name.clone(),
                        external_iid: link.pr_number.to_string(),
                        url: fields.url,
                        title: fields.title,
                        state: fields.state,
                        merged: fields.merged,
                        draft: fields.draft,
                        remote_updated_at: fields.remote_updated_at,
                        metadata: fields.metadata,
                    },
                    now,
                    actor_id,
                )
                .await?;
        }
    }
    Ok(())
}

/// Outcome of [`attach_pull_request`]: `(link, created)` plus the
/// snapshot warning (at most one — `best_effort_snapshot` logs once).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachOutcome {
    pub link: PrLinkRow,
    pub created: bool,
    pub warnings: Vec<String>,
}

/// Outcome of [`detach_pull_request_link`]: whether the mirror row was
/// also deleted, plus the instance-`delete()` fan-out enqueues in
/// Python order (link first, then mirror).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetachOutcome {
    pub review_deleted: bool,
    pub enqueues: Vec<TaskEnqueue>,
}

/// Attach (or idempotently re-attach) a PR to a work item
/// (`attach_pull_request`, `:119-174`).
///
/// `raw_url` strips (Python's `(raw_url or "").strip()`); the owner/name
/// lowercase after parsing (`:133`); `number` parses to the integer
/// column (an absurd digit run is a [`StoreError`] — Python would 500
/// with a `DataError` at INSERT). `actor_id` is the CRUM request user
/// for the audit columns. Returns `(link, created)` with the snapshot
/// warning, if any.
pub async fn attach_pull_request<S: PrLinksStore, C: GithubClient>(
    store: &S,
    project_id: Uuid,
    issue_id: Uuid,
    workspace_slug: &str,
    raw_url: &str,
    now: &chrono::DateTime<chrono::Utc>,
    actor_id: Option<Uuid>,
) -> Result<AttachOutcome, AttachError> {
    // `(raw_url or "").strip()`: the handler maps a missing URL to `""`.
    let parsed = parse_pr_url(raw_url).ok_or(AttachError::InvalidUrl(InvalidPullRequestURL))?;
    // GitHub owners/repos are case-insensitive; normalize so attach and
    // the webhook lookup agree (`:131-133`).
    let owner = parsed.owner.to_lowercase();
    let name = parsed.name.to_lowercase();
    let number: i64 = parsed.number.parse().map_err(|_| {
        AttachError::Store(StoreError(format!(
            "pr_number {} out of range for integer column",
            parsed.number
        )))
    })?;

    // The permission class only proves project membership; confirm the
    // work item actually belongs here (`:135-139`).
    if !store
        .issue_exists(issue_id, project_id, workspace_slug)
        .await?
    {
        return Err(AttachError::IssueNotFound(IssueNotFound));
    }

    if let Some(existing) = store.pr_link_by_pr(&owner, &name, number).await? {
        if existing.issue_id != issue_id {
            return Err(PullRequestAlreadyLinked {
                issue_id: existing.issue_id,
            }
            .into());
        }
        ensure_code_review_link(store, &existing, now, actor_id).await?;
        return Ok(AttachOutcome {
            link: existing,
            created: false,
            warnings: Vec::new(),
        });
    }

    if let Some(review) = store
        .review_link_by_pr(&owner, &name, &parsed.number)
        .await?
    {
        if review.issue_id != issue_id {
            return Err(PullRequestAlreadyLinked {
                issue_id: review.issue_id,
            }
            .into());
        }
    }

    let installation = store
        .installation_for_account(workspace_slug, &owner)
        .await?;
    let (snapshot, warning) =
        best_effort_snapshot::<C>(installation.as_ref(), &owner, &name, number);
    let new = NewPrLink {
        id: Uuid::new_v4(),
        project_id,
        issue_id,
        repo_owner: owner.clone(),
        repo_name: name.clone(),
        pr_number: number,
        url: format!("{GITHUB_HOST}/{owner}/{name}/pull/{number}"),
        title: snapshot.title,
        state: snapshot.state,
        merged: snapshot.merged,
        draft: snapshot.draft,
        pr_updated_at: snapshot.pr_updated_at,
    };
    let mut warnings = Vec::new();
    if let Some(line) = warning {
        warnings.push(line);
    }
    match store.create_pr_link(&new, now, actor_id).await? {
        CreatePrLinkOutcome::Created(link) => {
            ensure_code_review_link(store, &link, now, actor_id).await?;
            Ok(AttachOutcome {
                link: *link,
                created: true,
                warnings,
            })
        }
        // A concurrent attach won the partial-unique race; resolve to
        // the row that now exists (`:165-174`).
        CreatePrLinkOutcome::Conflict => {
            let existing = store.pr_link_by_pr(&owner, &name, number).await?;
            match existing {
                None => Err(AttachError::Store(StoreError(
                    "pr link lost the create race and the re-read missed".to_owned(),
                ))),
                Some(link) => {
                    if link.issue_id != issue_id {
                        return Err(PullRequestAlreadyLinked {
                            issue_id: link.issue_id,
                        }
                        .into());
                    }
                    ensure_code_review_link(store, &link, now, actor_id).await?;
                    Ok(AttachOutcome {
                        link,
                        created: false,
                        warnings,
                    })
                }
            }
        }
    }
}

/// Detach a PR link (`detach_pull_request_link`, `:112-116`): find the
/// matching mirror first, soft-delete the link, then soft-delete the
/// mirror too iff it points at the same issue. Each instance `delete()`
/// fans out [`SOFT_DELETE_TASK`] (returned as data for the executing
/// layer — link first, then mirror).
pub async fn detach_pull_request_link<S: PrLinksStore>(
    store: &S,
    link: &PrLinkRow,
    now: &chrono::DateTime<chrono::Utc>,
    actor_id: Option<Uuid>,
) -> Result<DetachOutcome, StoreError> {
    let review = store
        .review_link_by_pr(
            &link.repo_owner,
            &link.repo_name,
            &link.pr_number.to_string(),
        )
        .await?;
    store.delete_pr_link(link.id, now, actor_id).await?;
    let mut enqueues = vec![soft_delete_enqueue("githubpullrequestlink", &link.id)];
    let review_deleted = match review {
        Some(row) if row.issue_id == link.issue_id => {
            store.delete_review_link(row.id, now, actor_id).await?;
            enqueues.push(soft_delete_enqueue("gitcodereviewlink", &row.id));
            true
        }
        _ => false,
    };
    Ok(DetachOutcome {
        review_deleted,
        enqueues,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::adapters_github::GithubError;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// Load the FX-ISS-13 fixture.
    fn fixture() -> serde_json::Value {
        let path = format!(
            "{}/../../fixtures/app_issues/queries/FX-ISS-13.move.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&path).expect("FX-ISS-13 fixture exists");
        serde_json::from_str(&text).expect("FX-ISS-13 fixture parses")
    }

    fn uuid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("valid test clock")
    }

    fn link_row(issue: Uuid) -> PrLinkRow {
        PrLinkRow {
            id: uuid(0x11),
            project_id: uuid(0x21),
            issue_id: issue,
            repo_owner: "octo".to_owned(),
            repo_name: "repo".to_owned(),
            pr_number: 7,
            url: "https://github.com/octo/repo/pull/7".to_owned(),
            title: "Fix it".to_owned(),
            state: "open".to_owned(),
            merged: false,
            draft: false,
            pr_updated_at: None,
        }
    }

    /// Recording fake [`PrLinksStore`].
    struct FakeStore {
        calls: RefCell<Vec<String>>,
        issues: HashMap<(Uuid, Uuid, String), bool>,
        links: HashMap<(String, String, i64), PrLinkRow>,
        reviews: HashMap<(String, String, String), ReviewLinkRow>,
        installations: HashMap<(String, String), InstallationRow>,
        create_outcome: RefCell<Option<CreatePrLinkOutcome>>,
        created_reviews: RefCell<Vec<NewReviewLink>>,
        updated_reviews: RefCell<Vec<(Uuid, ReviewMirrorFields)>>,
        deleted_links: RefCell<Vec<Uuid>>,
        deleted_reviews: RefCell<Vec<Uuid>>,
    }

    impl FakeStore {
        fn empty() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                issues: HashMap::new(),
                links: HashMap::new(),
                reviews: HashMap::new(),
                installations: HashMap::new(),
                create_outcome: RefCell::new(None),
                created_reviews: RefCell::new(Vec::new()),
                updated_reviews: RefCell::new(Vec::new()),
                deleted_links: RefCell::new(Vec::new()),
                deleted_reviews: RefCell::new(Vec::new()),
            }
        }

        fn log(&self, call: String) {
            self.calls.borrow_mut().push(call);
        }
    }

    #[allow(async_fn_in_trait)]
    impl PrLinksStore for FakeStore {
        async fn installation_for_account(
            &self,
            workspace_slug: &str,
            owner: &str,
        ) -> Result<Option<InstallationRow>, StoreError> {
            self.log(format!("installation_for_account {workspace_slug} {owner}"));
            Ok(self
                .installations
                .get(&(workspace_slug.to_owned(), owner.to_owned()))
                .cloned())
        }

        async fn issue_exists(
            &self,
            issue_id: Uuid,
            project_id: Uuid,
            workspace_slug: &str,
        ) -> Result<bool, StoreError> {
            self.log("issue_exists".to_owned());
            Ok(*self
                .issues
                .get(&(issue_id, project_id, workspace_slug.to_owned()))
                .unwrap_or(&false))
        }

        async fn pr_link_by_pr(
            &self,
            owner: &str,
            name: &str,
            number: i64,
        ) -> Result<Option<PrLinkRow>, StoreError> {
            self.log(format!("pr_link_by_pr {owner}/{name}#{number}"));
            Ok(self
                .links
                .get(&(owner.to_owned(), name.to_owned(), number))
                .cloned())
        }

        async fn review_link_by_pr(
            &self,
            owner: &str,
            name: &str,
            number: &str,
        ) -> Result<Option<ReviewLinkRow>, StoreError> {
            self.log(format!("review_link_by_pr {owner}/{name}#{number}"));
            Ok(self
                .reviews
                .get(&(owner.to_owned(), name.to_owned(), number.to_owned()))
                .cloned())
        }

        async fn create_pr_link(
            &self,
            new: &NewPrLink,
            _now: &chrono::DateTime<chrono::Utc>,
            _actor_id: Option<Uuid>,
        ) -> Result<CreatePrLinkOutcome, StoreError> {
            self.log(format!(
                "create_pr_link {}/{}/{}",
                new.repo_owner, new.repo_name, new.pr_number
            ));
            Ok(self.create_outcome.borrow_mut().take().unwrap_or_else(|| {
                CreatePrLinkOutcome::Created(Box::new(PrLinkRow {
                    id: new.id,
                    project_id: new.project_id,
                    issue_id: new.issue_id,
                    repo_owner: new.repo_owner.clone(),
                    repo_name: new.repo_name.clone(),
                    pr_number: new.pr_number,
                    url: new.url.clone(),
                    title: new.title.clone(),
                    state: new.state.clone(),
                    merged: new.merged,
                    draft: new.draft,
                    pr_updated_at: new.pr_updated_at.clone(),
                }))
            }))
        }

        async fn create_review_link(
            &self,
            new: &NewReviewLink,
            _now: &chrono::DateTime<chrono::Utc>,
            _actor_id: Option<Uuid>,
        ) -> Result<ReviewLinkRow, StoreError> {
            self.log("create_review_link".to_owned());
            self.created_reviews.borrow_mut().push(new.clone());
            Ok(ReviewLinkRow {
                id: new.id,
                issue_id: new.issue_id,
                metadata: new.metadata.clone(),
            })
        }

        async fn update_review_link(
            &self,
            id: Uuid,
            fields: &ReviewMirrorFields,
            _now: &chrono::DateTime<chrono::Utc>,
            _actor_id: Option<Uuid>,
        ) -> Result<(), StoreError> {
            self.log("update_review_link".to_owned());
            self.updated_reviews.borrow_mut().push((id, fields.clone()));
            Ok(())
        }

        async fn delete_pr_link(
            &self,
            id: Uuid,
            _now: &chrono::DateTime<chrono::Utc>,
            _actor_id: Option<Uuid>,
        ) -> Result<(), StoreError> {
            self.log("delete_pr_link".to_owned());
            self.deleted_links.borrow_mut().push(id);
            Ok(())
        }

        async fn delete_review_link(
            &self,
            id: Uuid,
            _now: &chrono::DateTime<chrono::Utc>,
            _actor_id: Option<Uuid>,
        ) -> Result<(), StoreError> {
            self.log("delete_review_link".to_owned());
            self.deleted_reviews.borrow_mut().push(id);
            Ok(())
        }
    }

    /// Scriptable client: behaviour threads through a thread-local so the
    /// `connect` constructor (which takes no script) can still fail.
    struct ScriptedClient;

    thread_local! {
        static SCRIPT: RefCell<(Option<String>, Result<Value, String>)> =
            const { RefCell::new((None, Ok(Value::Null))) };
    }

    fn script_client(connect_error: Option<String>, pr_result: Result<Value, String>) {
        SCRIPT.with(|s| *s.borrow_mut() = (connect_error, pr_result));
    }

    impl GithubClient for ScriptedClient {
        fn connect(_auth: &ClientAuth) -> Result<Self, GithubError> {
            let failed = SCRIPT.with(|s| s.borrow().0.clone());
            match failed {
                Some(message) => Err(GithubError::Auth(message)),
                None => Ok(Self),
            }
        }

        fn get_authenticated_user(&self) -> Result<Value, GithubError> {
            Ok(Value::Null)
        }

        fn list_user_repos(&self, _page: i64) -> Result<(Vec<Value>, bool), GithubError> {
            Ok((Vec::new(), false))
        }

        fn get_repo(&self, _owner: &str, _name: &str) -> Result<Value, GithubError> {
            Ok(Value::Null)
        }

        fn list_all_open_issues(
            &self,
            _owner: &str,
            _name: &str,
        ) -> Result<Vec<Value>, GithubError> {
            Ok(Vec::new())
        }

        fn list_issue_comments(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
        ) -> Result<Vec<Value>, GithubError> {
            Ok(Vec::new())
        }

        fn post_issue_comment(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
            _body: &str,
        ) -> Result<Value, GithubError> {
            Ok(Value::Null)
        }

        fn get_pull_request(
            &self,
            _owner: &str,
            _name: &str,
            _number: i64,
        ) -> Result<Value, GithubError> {
            let result = SCRIPT.with(|s| s.borrow().1.clone());
            result.map_err(GithubError::Transport)
        }
    }

    // -- parse goldens ----------------------------------------------------

    #[test]
    fn parse_accepts_pr_urls() {
        let parsed = parse_pr_url("https://github.com/Octo/Repo/pull/7").expect("parses");
        // Case preserved here; attach lowercases.
        assert_eq!(parsed.owner, "Octo");
        assert_eq!(parsed.name, "Repo");
        assert_eq!(parsed.number, "7");
        // Leading zeros canonicalize; trailing paths allowed; http OK.
        assert_eq!(
            parse_pr_url("http://github.com/o/r/pull/007/files")
                .expect("parses")
                .number,
            "7"
        );
        assert_eq!(
            parse_pr_url("https://github.com/o/r/pull/0")
                .expect("parses")
                .number,
            "0"
        );
        // `.git` suffix kept (no suffix group in the PR regex).
        assert_eq!(
            parse_pr_url("https://github.com/o/r.git/pull/9")
                .expect("parses")
                .name,
            "r.git"
        );
        // Surrounding whitespace strips.
        assert!(parse_pr_url("  https://github.com/o/r/pull/1  ").is_some());
    }

    #[test]
    fn parse_rejects_non_pr_urls() {
        for bad in [
            "",
            "   ",
            "not-a-url",
            "https://github.com/octo/repo/issues/7",
            "https://github.com/octo/repo/pull/",
            "https://github.com/octo/repo/pull/abc",
            "https://github.com/octo/repo/pull/12x",
            "https://github.com//repo/pull/1",
            "https://github.com/octo//pull/1",
            "https://gitlab.com/octo/repo/pull/7",
            "https://github.com.evil.com/octo/repo/pull/7",
            "HTTPS://GITHUB.COM/octo/repo/pull/7",
            "git@github.com:octo/repo",
            "https://github.com/oc to/repo/pull/7",
            "https://github.com/octo/repo/pull/7/a b",
        ] {
            assert_eq!(parse_pr_url(bad), None, "{bad:?}");
        }
    }

    // -- snapshot goldens -------------------------------------------------

    #[test]
    fn snapshot_maps_payload() {
        let snap = pr_snapshot_from_payload(&serde_json::json!({
            "title": "Add the thing",
            "state": "closed",
            "merged": false,
            "merged_at": "2024-05-01T00:00:00Z",
            "draft": true,
            "updated_at": "2024-05-02T03:04:05Z",
        }));
        assert_eq!(snap.title, "Add the thing");
        assert_eq!(snap.state, "closed");
        assert!(snap.merged, "merged_at alone marks merged");
        assert!(snap.draft);
        assert_eq!(
            snap.pr_updated_at.as_deref(),
            Some("2024-05-02T03:04:05+00:00")
        );
    }

    #[test]
    fn snapshot_defaults_and_truncation() {
        let snap = pr_snapshot_from_payload(&Value::Null);
        assert_eq!(snap, default_snapshot());
        // Missing state renders open; falsy title renders "".
        assert_eq!(snap.state, "open");
        let long = "é".repeat(600);
        let snap = pr_snapshot_from_payload(&serde_json::json!({"title": long}));
        assert_eq!(snap.title.chars().count(), 500);
        // Non-string title renders "" (adapters precedent).
        let snap = pr_snapshot_from_payload(&serde_json::json!({"title": 5}));
        assert_eq!(snap.title, "");
        // Unparseable updated_at renders None, never an error.
        let snap = pr_snapshot_from_payload(&serde_json::json!({"updated_at": "not-a-date"}));
        assert_eq!(snap.pr_updated_at, None);
        // Truthiness follows Python bool(): 0/""/[]/{} are false.
        let snap = pr_snapshot_from_payload(&serde_json::json!({"merged": 0, "draft": []}));
        assert!(!snap.merged);
        assert!(!snap.draft);
        let snap = pr_snapshot_from_payload(&serde_json::json!({"merged": "yes"}));
        assert!(snap.merged);
    }

    #[test]
    fn mirror_state_map_matches_fixture() {
        let fx = fixture();
        assert!(
            fx["pr_links_goldens"]["ensure_code_review_mirror"]["state_map"]
                .as_str()
                .unwrap()
                .contains("merged->'merged'")
        );
        let merged = PrLinkRow {
            merged: true,
            state: "closed".to_owned(),
            ..link_row(uuid(1))
        };
        assert_eq!(mirror_state(&merged), "merged");
        let closed = PrLinkRow {
            state: "closed".to_owned(),
            ..link_row(uuid(1))
        };
        assert_eq!(mirror_state(&closed), "closed");
        assert_eq!(mirror_state(&link_row(uuid(1))), "open");
        let weird = PrLinkRow {
            state: "weird".to_owned(),
            ..link_row(uuid(1))
        };
        assert_eq!(mirror_state(&weird), "open");
    }

    // -- best-effort snapshot ----------------------------------------------

    #[test]
    fn best_effort_no_installation_is_empty_without_warning() {
        let (snap, warning) = best_effort_snapshot::<ScriptedClient>(None, "o", "r", 1);
        assert_eq!(snap, default_snapshot());
        assert_eq!(warning, None);
    }

    #[test]
    fn best_effort_failures_warn_with_the_message() {
        let installation = InstallationRow {
            id: uuid(0x31),
            installation_id: 42,
        };
        script_client(Some("bad credentials".to_owned()), Ok(Value::Null));
        let (snap, warning) =
            best_effort_snapshot::<ScriptedClient>(Some(&installation), "o", "r", 1);
        assert_eq!(snap, default_snapshot());
        assert_eq!(warning.as_deref(), Some("bad credentials"));
        script_client(None, Err("connection reset".to_owned()));
        let (snap, warning) =
            best_effort_snapshot::<ScriptedClient>(Some(&installation), "o", "r", 1);
        assert_eq!(snap, default_snapshot());
        assert_eq!(warning.as_deref(), Some("connection reset"));
    }

    #[test]
    fn best_effort_success_returns_snapshot() {
        let installation = InstallationRow {
            id: uuid(0x31),
            installation_id: 42,
        };
        script_client(None, Ok(serde_json::json!({"title": "T", "state": "open"})));
        let (snap, warning) =
            best_effort_snapshot::<ScriptedClient>(Some(&installation), "o", "r", 1);
        assert_eq!(snap.title, "T");
        assert_eq!(warning, None);
    }

    // -- attach ------------------------------------------------------------

    fn attach_store(issue: Uuid, project: Uuid) -> FakeStore {
        let mut store = FakeStore::empty();
        store
            .issues
            .insert((issue, project, "acme".to_owned()), true);
        store.installations.insert(
            ("acme".to_owned(), "octo".to_owned()),
            InstallationRow {
                id: uuid(0x31),
                installation_id: 42,
            },
        );
        store
    }

    #[tokio::test]
    async fn attach_success_creates_link_and_mirror() {
        let issue = uuid(1);
        let project = uuid(2);
        let store = attach_store(issue, project);
        script_client(None, Ok(serde_json::json!({"title": "T", "state": "open"})));
        let out = attach_pull_request::<FakeStore, ScriptedClient>(
            &store,
            project,
            issue,
            "acme",
            "https://github.com/Octo/Repo/pull/7",
            &now(),
            Some(uuid(9)),
        )
        .await
        .expect("attaches");
        assert!(out.created);
        assert!(out.warnings.is_empty());
        assert_eq!(out.link.repo_owner, "octo");
        assert_eq!(out.link.repo_name, "repo");
        assert_eq!(out.link.pr_number, 7);
        assert_eq!(out.link.url, "https://github.com/octo/repo/pull/7");
        assert_eq!(out.link.title, "T");
        // Call order mirrors Python: exists → link lookup → review lookup
        // → installation → create → mirror lookup → mirror create.
        assert_eq!(
            *store.calls.borrow(),
            vec![
                "issue_exists".to_owned(),
                "pr_link_by_pr octo/repo#7".to_owned(),
                "review_link_by_pr octo/repo#7".to_owned(),
                "installation_for_account acme octo".to_owned(),
                "create_pr_link octo/repo/7".to_owned(),
                "review_link_by_pr octo/repo#7".to_owned(),
                "create_review_link".to_owned(),
            ]
        );
        let created = &store.created_reviews.borrow();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].namespace, "octo");
        assert_eq!(created[0].external_iid, "7");
        assert_eq!(created[0].state, "open");
        assert_eq!(
            created[0].metadata,
            serde_json::json!({"legacy_github_pull_request_link_id": out.link.id.to_string()})
        );
    }

    #[tokio::test]
    async fn attach_rejects_bad_url_and_missing_issue() {
        let issue = uuid(1);
        let project = uuid(2);
        let store = attach_store(issue, project);
        let err = attach_pull_request::<FakeStore, ScriptedClient>(
            &store,
            project,
            issue,
            "acme",
            "https://github.com/octo/repo/issues/7",
            &now(),
            None,
        )
        .await
        .expect_err("bad url");
        assert_eq!(err, AttachError::InvalidUrl(InvalidPullRequestURL));
        assert!(store.calls.borrow().is_empty(), "no store touch on bad url");

        let err = attach_pull_request::<FakeStore, ScriptedClient>(
            &store,
            project,
            uuid(0x99),
            "acme",
            "https://github.com/octo/repo/pull/7",
            &now(),
            None,
        )
        .await
        .expect_err("missing issue");
        assert_eq!(err, AttachError::IssueNotFound(IssueNotFound));
        assert_eq!(*store.calls.borrow(), vec!["issue_exists".to_owned()]);
    }

    #[tokio::test]
    async fn attach_idempotent_and_already_linked_arms() {
        let issue = uuid(1);
        let project = uuid(2);
        let mut store = attach_store(issue, project);
        store
            .links
            .insert(("octo".to_owned(), "repo".to_owned(), 7), link_row(issue));
        // Same issue → ensure mirror, (link, False).
        let out = attach_pull_request::<FakeStore, ScriptedClient>(
            &store,
            project,
            issue,
            "acme",
            "https://github.com/octo/repo/pull/7",
            &now(),
            None,
        )
        .await
        .expect("re-attaches");
        assert!(!out.created);
        assert_eq!(out.link.id, uuid(0x11));
        assert!(store
            .calls
            .borrow()
            .contains(&"create_review_link".to_owned()));

        // Other issue → AlreadyLinked(existing.issue_id).
        let err = attach_pull_request::<FakeStore, ScriptedClient>(
            &store,
            project,
            uuid(0x99),
            "acme",
            "https://github.com/octo/repo/pull/7",
            &now(),
            None,
        )
        .await
        .expect_err("other issue");
        // NOTE: uuid(0x99) is not a member issue in this fake, so the
        // issue check fires first; register it and retry for the link arm.
        assert_eq!(err, AttachError::IssueNotFound(IssueNotFound));
        store
            .issues
            .insert((uuid(0x99), project, "acme".to_owned()), true);
        let err = attach_pull_request::<FakeStore, ScriptedClient>(
            &store,
            project,
            uuid(0x99),
            "acme",
            "https://github.com/octo/repo/pull/7",
            &now(),
            None,
        )
        .await
        .expect_err("other issue");
        assert_eq!(
            err,
            AttachError::AlreadyLinked(PullRequestAlreadyLinked { issue_id: issue })
        );

        // Mirror on another issue (no link row) → AlreadyLinked(review.issue_id).
        store.links.clear();
        store.reviews.insert(
            ("octo".to_owned(), "repo".to_owned(), "7".to_owned()),
            ReviewLinkRow {
                id: uuid(0x41),
                issue_id: uuid(0x77),
                metadata: Value::Object(Map::new()),
            },
        );
        let err = attach_pull_request::<FakeStore, ScriptedClient>(
            &store,
            project,
            issue,
            "acme",
            "https://github.com/octo/repo/pull/7",
            &now(),
            None,
        )
        .await
        .expect_err("mirror elsewhere");
        assert_eq!(
            err,
            AttachError::AlreadyLinked(PullRequestAlreadyLinked {
                issue_id: uuid(0x77)
            })
        );
    }

    #[tokio::test]
    async fn attach_race_resolves_to_existing_row() {
        let issue = uuid(1);
        let project = uuid(2);
        let mut store = attach_store(issue, project);
        *store.create_outcome.borrow_mut() = Some(CreatePrLinkOutcome::Conflict);
        store
            .links
            .insert(("octo".to_owned(), "repo".to_owned(), 7), link_row(issue));
        script_client(None, Ok(Value::Null));
        let out = attach_pull_request::<FakeStore, ScriptedClient>(
            &store,
            project,
            issue,
            "acme",
            "https://github.com/octo/repo/pull/7",
            &now(),
            None,
        )
        .await
        .expect("race resolves");
        assert!(!out.created);
        assert_eq!(out.link.id, uuid(0x11));

        // Race lost + re-read missed → re-raised as a store error.
        let store = attach_store(issue, project);
        *store.create_outcome.borrow_mut() = Some(CreatePrLinkOutcome::Conflict);
        script_client(None, Ok(Value::Null));
        let err = attach_pull_request::<FakeStore, ScriptedClient>(
            &store,
            project,
            issue,
            "acme",
            "https://github.com/octo/repo/pull/7",
            &now(),
            None,
        )
        .await
        .expect_err("missed re-read");
        assert!(matches!(err, AttachError::Store(_)));

        // Race lost to another issue → AlreadyLinked.
        let mut store = attach_store(issue, project);
        *store.create_outcome.borrow_mut() = Some(CreatePrLinkOutcome::Conflict);
        store.links.insert(
            ("octo".to_owned(), "repo".to_owned(), 7),
            link_row(uuid(0x55)),
        );
        script_client(None, Ok(Value::Null));
        let err = attach_pull_request::<FakeStore, ScriptedClient>(
            &store,
            project,
            issue,
            "acme",
            "https://github.com/octo/repo/pull/7",
            &now(),
            None,
        )
        .await
        .expect_err("race to other issue");
        assert_eq!(
            err,
            AttachError::AlreadyLinked(PullRequestAlreadyLinked {
                issue_id: uuid(0x55)
            })
        );
    }

    #[tokio::test]
    async fn attach_mirror_update_merges_metadata() {
        let issue = uuid(1);
        let project = uuid(2);
        let mut store = attach_store(issue, project);
        store
            .links
            .insert(("octo".to_owned(), "repo".to_owned(), 7), link_row(issue));
        store.reviews.insert(
            ("octo".to_owned(), "repo".to_owned(), "7".to_owned()),
            ReviewLinkRow {
                id: uuid(0x41),
                issue_id: issue,
                metadata: serde_json::json!({"keep": "me"}),
            },
        );
        let out = attach_pull_request::<FakeStore, ScriptedClient>(
            &store,
            project,
            issue,
            "acme",
            "https://github.com/octo/repo/pull/7",
            &now(),
            None,
        )
        .await
        .expect("re-attaches");
        assert!(!out.created);
        let updated = store.updated_reviews.borrow();
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].0, uuid(0x41));
        assert_eq!(
            updated[0].1.metadata,
            serde_json::json!({
                "keep": "me",
                "legacy_github_pull_request_link_id": uuid(0x11).to_string(),
            })
        );
        assert_eq!(updated[0].1.state, "open");
    }

    #[tokio::test]
    async fn attach_snapshot_failure_still_creates_with_defaults() {
        let issue = uuid(1);
        let project = uuid(2);
        let store = attach_store(issue, project);
        script_client(None, Err("boom".to_owned()));
        let out = attach_pull_request::<FakeStore, ScriptedClient>(
            &store,
            project,
            issue,
            "acme",
            "https://github.com/octo/repo/pull/7",
            &now(),
            None,
        )
        .await
        .expect("attaches despite snapshot failure");
        assert!(out.created);
        assert_eq!(out.warnings, vec!["boom".to_owned()]);
        assert_eq!(out.link.title, "");
        assert_eq!(out.link.state, "open");
    }

    // -- detach ------------------------------------------------------------

    #[tokio::test]
    async fn detach_deletes_link_and_same_issue_mirror() {
        let issue = uuid(1);
        let mut store = FakeStore::empty();
        let review_id = uuid(0x41);
        store.reviews.insert(
            ("octo".to_owned(), "repo".to_owned(), "7".to_owned()),
            ReviewLinkRow {
                id: review_id,
                issue_id: issue,
                metadata: Value::Object(Map::new()),
            },
        );
        let out = detach_pull_request_link(&store, &link_row(issue), &now(), Some(uuid(9)))
            .await
            .expect("detaches");
        assert!(out.review_deleted);
        assert_eq!(*store.deleted_links.borrow(), vec![uuid(0x11)]);
        assert_eq!(*store.deleted_reviews.borrow(), vec![review_id]);
        assert_eq!(
            *store.calls.borrow(),
            vec![
                "review_link_by_pr octo/repo#7".to_owned(),
                "delete_pr_link".to_owned(),
                "delete_review_link".to_owned(),
            ]
        );
        // Fan-out order: link first, then mirror.
        assert_eq!(out.enqueues.len(), 2);
        assert_eq!(out.enqueues[0].task, SOFT_DELETE_TASK);
        assert_eq!(
            out.enqueues[0].args,
            vec![
                Value::String("db".to_owned()),
                Value::String("githubpullrequestlink".to_owned()),
                Value::String(uuid(0x11).to_string()),
            ]
        );
        assert_eq!(out.enqueues[0].kwargs.get("using"), Some(&Value::Null));
        assert_eq!(
            out.enqueues[1].args[1],
            Value::String("gitcodereviewlink".to_owned())
        );
    }

    #[tokio::test]
    async fn detach_keeps_other_issue_mirror() {
        let mut store = FakeStore::empty();
        store.reviews.insert(
            ("octo".to_owned(), "repo".to_owned(), "7".to_owned()),
            ReviewLinkRow {
                id: uuid(0x41),
                issue_id: uuid(0x77),
                metadata: Value::Object(Map::new()),
            },
        );
        let out = detach_pull_request_link(&store, &link_row(uuid(1)), &now(), None)
            .await
            .expect("detaches");
        assert!(!out.review_deleted);
        assert!(store.deleted_reviews.borrow().is_empty());
        assert_eq!(out.enqueues.len(), 1);

        // No mirror at all: link delete only.
        let store = FakeStore::empty();
        let out = detach_pull_request_link(&store, &link_row(uuid(1)), &now(), None)
            .await
            .expect("detaches");
        assert!(!out.review_deleted);
        assert_eq!(out.enqueues.len(), 1);
    }

    // -- SQL shape ----------------------------------------------------------

    #[test]
    fn sql_texts_carry_manager_scope_and_ordering() {
        for sql in [
            PR_LINK_LOOKUP_SQL,
            REVIEW_LINK_LOOKUP_SQL,
            INSTALLATION_LOOKUP_SQL,
        ] {
            assert!(sql.contains("\"deleted_at\" IS NULL"), "{sql}");
            assert!(sql.contains("ORDER BY"), "{sql}");
            assert!(sql.contains("LIMIT 1"), "{sql}");
        }
        assert!(INSTALLATION_LOOKUP_SQL
            .contains("LOWER(\"github_app_installations\".\"account_login\") = LOWER($2)"));
        assert!(INSTALLATION_LOOKUP_SQL.contains("\"workspaces\".\"slug\" = $1"));
        assert!(ISSUE_EXISTS_SQL.contains("SELECT 1 AS \"a\""));
        assert!(PR_LINK_INSERT_SQL.contains("\"pr_number\""));
        assert!(REVIEW_LINK_INSERT_SQL.contains("'', '', $17"));
        assert!(REVIEW_LINK_UPDATE_SQL.contains("\"metadata\" = $12"));
        assert!(PR_LINK_SOFT_DELETE_SQL.contains("\"deleted_at\" = $1"));
    }
}
