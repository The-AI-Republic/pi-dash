#![forbid(unsafe_code)]

//! Legacy GitHub issue sync poller (D-05, jobs layer).
//!
//! Ports `apps/api/pi_dash/bgtasks/github_sync_task.py:48-389`
//! (translation only): the seven helpers (`_is_enabled`, `_resolve_token`,
//! `_project_default_state`, `_markdown_to_html`, `_safe_render`,
//! `pi_dash_issue_url`, `_upsert_issue`, `_upsert_comment`,
//! `_reconcile_upstream_gone`) and the three Celery tasks
//! (`sync_all_repos`, `sync_one_repo`, `post_completion_comment`).
//!
//! This module owns the canonical render helpers: `git_sync_task.py:15`
//! imports `_safe_render` and `pi_dash_issue_url` from here, so the
//! provider-neutral poller
//! ([`super::git_sync`], PIDASHCONV-147) reuses the beauty pipeline from
//! that side. To keep the two copies byte-identical by construction, the
//! generic pieces already ported there are imported here, not forked:
//! [`safe_render`][git_safe_render], [`strip_html_text`][git_strip],
//! [`truncate_chars`][git_truncate], [`json_truthy`][git_truthy],
//! [`pidash_issue_url`][git_url], [`completion_base_url`][git_base],
//! [`completion_body`][git_body], [`retry_countdown_secs`][git_retry],
//! [`advisory_lock_key`][git_lock], [`parse_remote_dt`][git_dt],
//! [`parse_single_id_arg`][git_arity], [`default_state_sql`][git_state],
//! [`save_state_sql`][git_save_state], [`next_link`][git_link],
//! [`GITHUB_API_BASE`][git_api], [`PROVIDER_TIMEOUT_SECS`][git_timeout],
//! [`installation_token`][git_token] and the error-text builders.
//!
//! [git_safe_render]: super::git_sync::safe_render
//! [git_strip]: super::git_sync::strip_html_text
//! [git_truncate]: super::git_sync::truncate_chars
//! [git_truthy]: super::git_sync::json_truthy
//! [git_url]: super::git_sync::pidash_issue_url
//! [git_base]: super::git_sync::completion_base_url
//! [git_body]: super::git_sync::completion_body
//! [git_retry]: super::git_sync::retry_countdown_secs
//! [git_lock]: super::git_sync::advisory_lock_key
//! [git_dt]: super::git_sync::parse_remote_dt
//! [git_arity]: super::git_sync::parse_single_id_arg
//! [git_state]: super::git_sync::default_state_sql
//! [git_save_state]: super::git_sync::save_state_sql
//! [git_link]: super::git_sync::next_link
//! [git_api]: super::git_sync::GITHUB_API_BASE
//! [git_timeout]: super::git_sync::PROVIDER_TIMEOUT_SECS
//! [git_token]: super::git_sync::installation_token
//!
//! Layering: the table shapes live in `pidash-db` (stage 5 models,
//! `github_models.rs`); the PAT error triad in `pidash-services`
//! (`adapters_github::GithubError`, PIDASHCONV-141). This module owns the
//! task bodies, the Celery wire payloads, the retry policy, and the live
//! transport ([`GithubRestClient`]).
//!
//! Unlike the provider-neutral poller, this task talks raw GitHub REST
//! (`pi_dash.utils.github_client.GithubClient`, `github_client.py:38-220`)
//! instead of the adapter DTOs: issues arrive as plain dicts, PRs filter by
//! `pull_request` key presence, and comments enumerate repo-wide with the
//! parent resolved through [`parse_issue_number_from_url`]. The services
//! [`GithubClient`][svc_client] trait has no repo-wide comments endpoint,
//! so the transport lives here (foundation crates are read-only); the
//! header/status/`Link` mapping is transcribed arm for arm from
//! `github_client.py:54-96`.
//!
//! [svc_client]: pidash_services::integrations::adapters_github::GithubClient
//!
//! SQL semantics mirror the Django ORM arm for arm (see each builder):
//! soft-delete scoping follows the default managers; `select_related`
//! joins are plain `INNER JOIN`s with no tombstone filter on the joined
//! tables, exactly like Django; `update_or_create` is select-then-insert
//! (with the full `save()` side effects) or setattr-plus-`save()` — not
//! `filter().update` — so the update arm re-runs `Issue.save`
//! (state/completed-at/description recompute) before the audit restamp.
//!
//! Redelivery safety: every write is keyed on a natural unique
//! (`project + external_source + external_id`, `repository_sync + issue`,
//! `issue + external_source + external_id`, `issue_sync + comment`) via
//! select-then-insert-or-update — the same race window as Python's
//! `update_or_create` (a concurrent duplicate errors and retries, never
//! double-applies). Queue-level dedup comes from the F-09 plane: a failure
//! requeues with [`Verdict::Retry`], a spent budget parks as `failed`;
//! nothing is ever dropped ([`crate::worker`]).
//!
//! PORT BUGS (ported, not fixed):
//!
//! * `_ = default_state`: `_upsert_issue` ignores its `default_state`
//!   argument (`github_sync_task.py:166`); the new row's state comes from
//!   `Issue.save()` re-deriving it (non-triage default). The parameter is
//!   still resolved (and its lookup failures still fail the task) so the
//!   observable behavior matches.
//! * The save-time state query filters `is_triage = false` while
//!   `_project_default_state` excludes `group = 'triage'` instead — two
//!   different notions of "triage" that agree on seeded data (same
//!   inherited quirk as the provider-neutral poller).
//! * On the issue UPDATE arm, `update_or_create` runs the full
//!   `Issue.save()`: a mirror sitting in a completed-group state gets
//!   `completed_at` re-stamped to now, and any other mirror gets
//!   `completed_at` cleared — clobbering the real completion time on every
//!   re-sync (`issue.py:302-309`). [`upsert_issue`] mirrors it.
//! * The `.strip()` on the mirror `comment_stripped`
//!   (`github_sync_task.py:205`) is dead: `update_or_create` always runs
//!   the full `IssueComment.save()`, which recomputes the column from
//!   `comment_html` without stripping (`issue.py:606`).
//! * The new-comment `Description` row keeps entities (`Description.save`
//!   re-strips with Django semantics, `description.py:22-28`) while the
//!   description refresh on the update path writes the decoded recompute
//!   (`filter().update`, no `save()`).
//! * `GithubIssueSync` / `GithubCommentSync` audit columns land NULL:
//!   `update_or_create`'s `save()` re-stamps them from crum (no request
//!   user in the worker) and, unlike `Issue` / `IssueComment`, there is no
//!   `.update()` restamp afterwards (`github_sync_task.py:168-188,
//!   :230-240`). On the update arm the NULLs additionally clobber the
//!   values a previous arm wrote.
//! * The metadata read-modify-writes (`_reconcile_upstream_gone`, the
//!   completion guard, the login merge) are non-atomic, as in Python.
//! * A truthy non-string `config["token"]` raises out of `_resolve_token`
//!   (outside the guarded `try`, so the task fails without recording);
//!   falsy shapes take the missing-credential path via `or ""`, and a
//!   failed decryption returns `""`, which takes it too.
//!
//! Deliberate approximations (unreachable with real GitHub payloads,
//! documented here instead of a paragraph per call site):
//!
//! * Non-integer `id` fields coerce to `0` (`github_issue_id`,
//!   `repo_comment_id` on the sync rows): Python would keep a truthy
//!   non-string through `or 0` and let the column coerce-or-raise, while
//!   a missing `number`/`comment id` still errors into record-and-retry.
//!   A present-but-non-integer comment `id` likewise retries here where
//!   Python would mirror it under `str(id)`. GitHub always sends integers
//!   here.
//! * `issue_url` keeps strings verbatim and renders everything else as
//!   `""`; Python would `str()` a truthy non-string. Same reachability.
//! * A non-string comment `issue_url`, a non-object `user`, or a
//!   non-string `user.login` skips the comment/login merge here, where
//!   Python would raise into record-and-retry (or store the odd value).
//!   GitHub always sends a string URL, an object user, and a string login.
//! * `\d`'s Unicode tail in `parse_issue_number_from_url` yields `None`
//!   here (ASCII digits only); GitHub URLs are ASCII.
//!
//! Fixture ids replayed by the unit tests below:
//! `rust-api/fixtures/integrations/tasks/github_sync_task.before_after.json`
//! (the issue names it `tasks/github_sync.json`; the recorded behavior
//! lives under the `github_sync_task.before_after.json` name) and
//! `rust-api/fixtures/integrations/tasks/completion_guards.golden.json`
//! (the issue names it `tasks/signals.json`).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_db::config::encryption::Keyring;
use pidash_services::integrations::adapters_github::GithubError;

use super::super::tasks_mail::mail_send::strip_tags;
use super::git_sync::{
    advisory_lock_key, completion_base_url, completion_body, completion_error_text,
    default_state_sql, json_truthy, next_link, parse_remote_dt, parse_single_id_arg,
    pidash_issue_url, retry_countdown_secs, safe_render, save_state_sql, strip_html_text,
    truncate_chars, GITHUB_API_BASE, PROVIDER_TIMEOUT_SECS,
};
use crate::celery::{format_eta, CeleryTaskMessage};
use crate::queue::{enqueue, JobRow, NewJob};
use crate::worker::{HandlerError, Registry, Verdict};

// ---------------------------------------------------------------------------
// Task identity
// ---------------------------------------------------------------------------

/// `sync_all_repos` (`github_sync_task.py:264`, plain `@shared_task`:
/// never retries — fan-out failures park, and the next beat tick refires).
pub const SYNC_ALL_REPOS_TASK: &str = "pi_dash.bgtasks.github_sync_task.sync_all_repos";
/// `sync_one_repo` (`github_sync_task.py:274`, `bind=True,
/// max_retries=3`).
pub const SYNC_ONE_REPO_TASK: &str = "pi_dash.bgtasks.github_sync_task.sync_one_repo";
/// `post_completion_comment` (`github_sync_task.py:338`, plain
/// `@shared_task`: never retries — every fault is recorded and returned).
pub const POST_COMPLETION_COMMENT_TASK: &str =
    "pi_dash.bgtasks.github_sync_task.post_completion_comment";

/// Every Celery task name this module owns, in Python definition order.
pub const TASK_NAMES: [&str; 3] = [
    SYNC_ALL_REPOS_TASK,
    SYNC_ONE_REPO_TASK,
    POST_COMPLETION_COMMENT_TASK,
];

/// `bind=True, max_retries=3` (`github_sync_task.py:274`). The queue row
/// carries the same budget: [`NewJob::new`] defaults to
/// [`DEFAULT_MAX_RETRIES`], which is 3 for exactly this task.
pub const MAX_RETRIES: i32 = 3;

// ---------------------------------------------------------------------------
// Kill switch
// ---------------------------------------------------------------------------

/// Instance-level kill switch (`github_sync_task.py:48-50`):
/// `getattr(settings, "GITHUB_SYNC_ENABLED", True)`. Unlike the
/// provider-neutral poller there is no `GIT_SYNC_ENABLED` first lookup —
/// this task reads exactly one setting.
pub fn github_sync_enabled() -> bool {
    std::env::var("GITHUB_SYNC_ENABLED")
        .ok()
        .map(|value| value.to_lowercase() == "true")
        .unwrap_or(true)
}

// ---------------------------------------------------------------------------
// Small pure helpers
// ---------------------------------------------------------------------------

/// Mirror title: `f"[github_{number}] {title}"[:255]`
/// (`github_sync_task.py:123`). Truncation is by code point, never bytes.
pub fn github_issue_name(number: i64, title: &str) -> String {
    truncate_chars(&format!("[github_{number}] {title}"), 255)
}

/// Coerce `gh_issue.get("title") or ""` (`github_sync_task.py:122`):
/// falsy (missing/null/empty/false/zero) renders `""`; strings verbatim;
/// `True` renders Python's `"True"`; other numbers render as-is.
/// Containers render `""` (unreachable with real GitHub payloads, where
/// the field is always a string or missing).
pub fn coerce_title(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(true)) => "True".to_owned(),
        Some(Value::Bool(false)) => String::new(),
        Some(Value::Number(number)) => {
            if json_truthy(&Value::Number(number.clone())) {
                number.to_string()
            } else {
                String::new()
            }
        }
        Some(Value::Array(_)) | Some(Value::Object(_)) => String::new(),
    }
}

/// Extract a renderable body (`gh_*.get("body")`,
/// `github_sync_task.py:124,201`): missing/null is `None`; strings pass
/// through; any other truthy JSON type errors — Python would call
/// `.split`/truthiness on it and raise `AttributeError` inside the guarded
/// `try`, i.e. the record-and-retry path, which is what the `Err` below
/// feeds.
pub fn coerce_body(value: Option<&Value>) -> Result<Option<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(other) => Err(format!("upstream body is not a string: {other}")),
    }
}

/// Mirror comment HTML (`github_sync_task.py:204`): leading paragraph form
/// so multi-paragraph upstream bodies aren't broken by an inline prefix.
pub fn mirror_comment_html(safe_html: &str) -> String {
    format!("<p>[Github] </p>{safe_html}")
}

/// The stored stripped form: `IssueComment.save()` recomputes
/// `comment_stripped = strip_tags(comment_html)` (`issue.py:606`,
/// html_processor semantics), so the database keeps e.g. `"[Github] "`
/// for empty bodies — the `f"[Github] {stripped}".strip()`
/// (`github_sync_task.py:205`) never reaches the database (ported bug,
/// listed in the module docs).
pub fn mirror_comment_stripped(comment_html: &str) -> String {
    strip_html_text(comment_html)
}

/// `f"{TypeName}: {msg[:900]}"` for scan error recording
/// (`github_sync_task.py:329`): `GithubAuthError` /
/// `GithubPermissionError` / `GithubNotFoundError`. Only the message is
/// truncated, the label is prepended whole.
pub fn scan_error_label(error: &GithubError) -> &'static str {
    match error {
        GithubError::Auth(_) => "GithubAuthError",
        GithubError::Permission(_) => "GithubPermissionError",
        GithubError::NotFound(_) => "GithubNotFoundError",
        GithubError::Transport(_) => "GithubError",
    }
}

/// `f"{TypeName}: {msg[:900]}"` (`github_sync_task.py:329`).
pub fn scan_error_text(error: &GithubError) -> String {
    format!(
        "{}: {}",
        scan_error_label(error),
        truncate_chars(error.message(), 900),
    )
}

/// Missing-credential record (`github_sync_task.py:288`).
pub const MISSING_TOKEN_ERROR: &str =
    "GitHub credential is missing or workspace integration disconnected";

/// Missing-credential metadata record on the completion path
/// (`github_sync_task.py:360`).
pub const COMPLETION_MISSING_TOKEN_ERROR: &str = "credential missing or disconnected";

/// Resolve the PAT (`_resolve_token`, `github_sync_task.py:53-58`):
/// `config or {}` → `config.get("token") or ""` → `decrypt_data`.
/// Every falsy shape (missing, null, `""`, `false`, `0`, `[]`, `{}`)
/// yields `""` (the callers treat it as missing), like Python's
/// `or ""`; a failed decryption yields `""` too (`decrypt_data` fails
/// closed to `""`). A truthy non-string token errors — Python's
/// `.encode()` raises outside the guarded `try`, failing the task
/// without recording.
pub fn resolve_token(config: &Value, keyring: &Keyring) -> Result<String, String> {
    let token = match config.as_object().and_then(|map| map.get("token")) {
        None => String::new(),
        Some(value) if !json_truthy(value) => String::new(),
        Some(Value::String(text)) => keyring.decrypt(text),
        Some(other) => {
            return Err(format!(
                "workspace integration token is not a string: {other}"
            ));
        }
    };
    Ok(token)
}

/// Extract the issue number from a GitHub comment's `issue_url` field
/// (`parse_issue_number_from_url`, `github_client.py:220-228`):
/// `re.search(r"/repos/[^/]+/[^/]+/issues/(\d+)$")`. Falsy input yields
/// `None`. The `regex` crate has no lookaround gap here — the pattern is
/// a plain search — but a hand roll keeps this module dependency-free and
/// mirrors the match arm for arm: leftmost `/repos/` whose remainder is
/// exactly `<seg>/<seg>/issues/<digits>` (nonempty slash-free segments,
/// ASCII digits; `\d`'s Unicode tail is unreachable with real GitHub
/// URLs and yields `None` here, documented as an approximation).
pub fn parse_issue_number_from_url(issue_url: Option<&str>) -> Option<i64> {
    let url = issue_url.filter(|text| !text.is_empty())?;
    let mut rest = url;
    while let Some(start) = rest.find("/repos/") {
        let tail = &rest[start + "/repos/".len()..];
        let parts: Vec<&str> = tail.split('/').collect();
        if parts.len() == 4
            && !parts[0].is_empty()
            && !parts[1].is_empty()
            && parts[2] == "issues"
            && !parts[3].is_empty()
            && parts[3].bytes().all(|byte| byte.is_ascii_digit())
        {
            return parts[3].parse::<i64>().ok();
        }
        rest = &rest[start + 1..];
    }
    None
}

// ---------------------------------------------------------------------------
// SQL (mirrors the ORM call-for-call; `$n` binds filled by the handlers)
// ---------------------------------------------------------------------------

/// Enabled-sync id set (`github_sync_task.py:269`):
/// `GithubRepositorySync.objects.filter(is_sync_enabled=True)` — the
/// default manager scopes `deleted_at IS NULL`; no ordering.
pub const ENABLED_SYNCS_SQL: &str =
    "SELECT id FROM github_repository_syncs WHERE is_sync_enabled = TRUE AND deleted_at IS NULL";

/// One sync with its `select_related` closure
/// (`github_sync_task.py:280-282`): repository, workspace_integration,
/// project. `select_related` joins are plain `INNER JOIN`s — Django does
/// NOT apply the related managers' tombstone filters here, so neither
/// does this statement: only the base sync row is soft-delete scoped.
/// `$1` is the sync id; a miss is the `DoesNotExist` no-op. The project
/// join selects no columns (only `sync.project_id` is ever read), so it
/// is omitted with identical observable behavior.
pub const SYNC_SCAN_SQL: &str =
    "SELECT s.id AS sync_id, s.project_id, s.workspace_id, s.actor_id, \
    r.owner AS repo_owner, r.name AS repo_name, \
    wi.config AS integration_config \
    FROM github_repository_syncs s \
    INNER JOIN github_repositories r ON r.id = s.repository_id \
    INNER JOIN workspace_integrations wi ON wi.id = s.workspace_integration_id \
    WHERE s.id = $1 AND s.deleted_at IS NULL";

/// The project workspace for the create path (`ProjectBaseModel.save`,
/// `project.py:309-311`): `self.workspace = self.project.workspace`.
/// `$1` is the project id.
pub const PROJECT_WORKSPACE_SQL: &str = "SELECT workspace_id FROM projects WHERE id = $1";

/// Mirror lookup (`github_sync_task.py:135-140`):
/// `Issue.objects.update_or_create(project, external_source="github",
/// external_id, …)` — the default manager scopes `deleted_at IS NULL`.
/// `$1` the project id, `$2` the upstream issue number.
pub const ISSUE_LOOKUP_SQL: &str = "SELECT id FROM issues WHERE project_id = $1 AND external_source = 'github' AND external_id = $2 AND deleted_at IS NULL";

/// Current state + group + stored completion stamp for the issue UPDATE
/// arm (`Issue.save`, `issue.py:288-309`): the row's own state, if any.
/// `$1` is the issue id. A NULL state re-resolves the default (same
/// branch as creation); the stored `completed_at` rides along because
/// the `state is None` branch never touches it.
pub const ISSUE_STATE_SQL: &str = "SELECT i.state_id, s.\"group\" AS state_group, i.completed_at FROM issues i LEFT JOIN states s ON s.id = i.state_id WHERE i.id = $1";

/// Adopt the resolved default on a stateless mirror (`Issue.save`,
/// `issue.py:288-301`): `self.state = default_state` then the full
/// `save()` persists it. `$1` the issue id, `$2` the resolved state id
/// (nullable — no default exists, the row stays stateless, as in Python).
const ADOPT_STATE_SQL: &str = "UPDATE issues SET state_id = $2 WHERE id = $1";

/// `Issue.objects.create(…)` column list, in physical order: the
/// `Issue.save()` creation path resolves pod/state/sequence/sort-order
/// first (see [`create_issue`]), then writes the full row. Application
/// defaults with no DB fallback (`priority 'none'`, `complexity_score 0`,
/// `sequence/sort` resolved, `is_draft false`, `git_work_branch ''`,
/// `workpad ''`); everything else NULL. `workspace_id` comes from
/// `ProjectBaseModel.save` (`project.workspace`), not the task defaults.
const INSERT_ISSUE_SQL: &str = "INSERT INTO issues (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, parent_id, state_id, point, estimate_point_id, name, description_json, description_html, description_stripped, description_binary, priority, complexity_score, start_date, target_date, sequence_id, sort_order, completed_at, archived_at, is_draft, external_source, external_id, type_id, git_work_branch, workpad, created_via, assigned_pod_id, agent_executor) VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, NULL, $8, NULL, NULL, $9, $10, $11, $12, NULL, 'none', 0, NULL, NULL, $13, $14, $15, NULL, FALSE, $16, $17, NULL, '', '', NULL, $18, NULL)";

/// `update_or_create` issue UPDATE arm (`github_sync_task.py:135-140` via
/// `Issue.save`, `issue.py:288-350`): `save()` rewrites the whole row,
/// but only the render triple, the recomputed `completed_at` and the
/// audit restamp can differ — `name`, state and `updated_at` (auto_now)
/// aside, every other column keeps its value. (The two Python writes —
/// `save()` then `filter(pk).update(created_by, updated_by)` — collapse
/// into this one statement; the final bytes are identical.)
const UPDATE_ISSUE_SQL: &str = "UPDATE issues SET name = $2, description_html = $3, description_stripped = $4, description_json = $5, created_by_id = $6, updated_by_id = $6, completed_at = $7, updated_at = $8 WHERE id = $1";

/// `GithubIssueSync.objects.update_or_create(repository_sync, issue, …)`
/// (`github_sync_task.py:168-182`): full-row insert and full-defaults
/// update (plus `updated_at`, which `save()` touches on the update path).
/// Audit lands NULL on both arms (`BaseModel.save` crum clobber, no
/// restamp — ported bug, see the module docs). `metadata` is NOT in the
/// defaults: creates start at `{}`, updates keep the stored value.
const INSERT_ISSUE_SYNC_SQL: &str = "INSERT INTO github_issue_syncs (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, repo_issue_id, github_issue_id, issue_url, issue_id, repository_sync_id, metadata, gh_issue_created_at, gh_issue_updated_at) VALUES ($1, $2, $3, NULL, NULL, NULL, $4, $5, $6, $7, $8, $9, $10, '{}', $11, $12)";
const UPDATE_ISSUE_SYNC_SQL: &str = "UPDATE github_issue_syncs SET repo_issue_id = $2, github_issue_id = $3, issue_url = $4, workspace_id = $5, project_id = $6, created_by_id = NULL, updated_by_id = NULL, gh_issue_created_at = $7, gh_issue_updated_at = $8, updated_at = $9 WHERE id = $1";

/// Mirror-sync lookup for the upsert
/// (`github_sync_task.py:168-169`): `update_or_create(repository_sync,
/// issue)` — metadata is read for the login merge below. `$1` the sync
/// id, `$2` the issue id.
pub const GITHUB_SYNC_LOOKUP_SQL: &str = "SELECT id, metadata FROM github_issue_syncs WHERE repository_sync_id = $1 AND issue_id = $2 AND deleted_at IS NULL";

/// `IssueSequence.objects.create(issue, sequence, project)`
/// (`issue.py:340`): `ProjectBaseModel` audit columns, `deleted false`.
/// `workspace_id` derives from `project.workspace` via
/// `ProjectBaseModel.save`, same as the issue row.
const INSERT_ISSUE_SEQUENCE_SQL: &str = "INSERT INTO issue_sequences (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, issue_id, sequence, deleted) VALUES ($1, $2, $3, NULL, NULL, NULL, $4, $5, $6, $7, FALSE)";

/// Next per-project sequence (`Issue.save`, `issue.py:314-327`): the max
/// over the scoped `IssueSequence` rows. `$1` is the project id; NULL
/// means the first issue (`sequence_id = 1`).
pub const MAX_SEQUENCE_SQL: &str =
    "SELECT MAX(sequence) FROM issue_sequences WHERE project_id = $1 AND deleted_at IS NULL";

/// Sort-order seed (`Issue.save`, `issue.py:331-335`): the max over live
/// sibling rows in the same resolved state (`state=None` matches
/// `state_id IS NULL`, via `IS NOT DISTINCT FROM`). `$1` the project id,
/// `$2` the resolved state id (nullable). NULL keeps the field default
/// `65535`.
pub const MAX_SORT_ORDER_SQL: &str = "SELECT MAX(sort_order) FROM issues WHERE project_id = $1 AND state_id IS NOT DISTINCT FROM $2 AND deleted_at IS NULL";

/// Take the per-project creation lock (`issue.py:318-320`):
/// `SELECT pg_advisory_xact_lock(%s)`. Runs inside the creation
/// transaction, so the lock releases on commit/rollback like Python's
/// `transaction.atomic()` block.
pub const ADVISORY_LOCK_SQL: &str = "SELECT pg_advisory_xact_lock($1)";

/// Default-pod resolution (`Issue.save`, `issue.py:277-286`):
/// `Pod.objects.filter(project_id, is_default=True).first()` — live rows
/// only (`PodManager`), Meta ordering `(-is_default, created_at)` which is
/// `created_at ASC` once every candidate is default. `$1` is the project
/// id. A miss leaves `assigned_pod_id` NULL.
pub const DEFAULT_POD_SQL: &str = "SELECT id FROM pod WHERE project_id = $1 AND is_default = TRUE AND deleted_at IS NULL ORDER BY created_at ASC LIMIT 1";

/// Mirror-comment lookup (`github_sync_task.py:207-221`):
/// `IssueComment.objects.update_or_create(issue, external_source,
/// external_id, …)` — the existing row's render columns feed the
/// change-tracker comparison for the description side-table. `$1` the
/// issue id, `$2` the upstream comment id.
pub const ISSUE_COMMENT_LOOKUP_SQL: &str = "SELECT id, comment_html, comment_stripped, comment_json, description_id FROM issue_comments WHERE issue_id = $1 AND external_source = 'github' AND external_id = $2 AND deleted_at IS NULL";

/// `Description.objects.create(…)` for a new mirror comment
/// (`issue.py:625`): `WorkspaceBaseModel` columns; `description_stripped`
/// recomputed by `Description.save` — which imports DJANGO's `strip_tags`
/// (`description.py:6`, entities kept), overwriting the decoded value the
/// caller passed (`description.py:22-28`). Hence the Django
/// [`strip_tags`] call here, unlike every other stripped write in this
/// module.
const INSERT_DESCRIPTION_SQL: &str = "INSERT INTO descriptions (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, project_id, description_json, description_html, description_binary, description_stripped) VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, $9, NULL, $10)";

/// `IssueComment.objects.create` column list: array defaults `'{}'`,
/// `access 'INTERNAL'`, `speaker_type 'human'`, `speaker_label ''`.
const INSERT_ISSUE_COMMENT_SQL: &str = "INSERT INTO issue_comments (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, comment_stripped, comment_json, comment_html, description_id, attachments, labels, issue_id, actor_id, access, external_source, external_id, speaker_type, speaker_label, speaker_agent_run_id, edited_at, parent_id) VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, $9, $10, $11, '{}', '{}', $12, $13, 'INTERNAL', 'github', $14, 'human', '', NULL, NULL, NULL)";

/// Existing mirror comment refresh (`issue.py:606-646`): the render
/// triple plus audit restamp and `updated_at` (full `save()`); the
/// description side-table updates only when a tracked field changed.
const UPDATE_ISSUE_COMMENT_SQL: &str = "UPDATE issue_comments SET comment_html = $2, comment_stripped = $3, comment_json = $4, workspace_id = $5, project_id = $6, actor_id = $7, created_by_id = $8, updated_by_id = $8, updated_at = $9 WHERE id = $1";
const UPDATE_COMMENT_DESCRIPTION_SQL: &str = "UPDATE descriptions SET description_html = $2, description_stripped = $3, description_json = $4, updated_by_id = $5, updated_at = $6 WHERE id = $1";

/// Comment-sync lookup (`github_sync_task.py:230-232`):
/// `GithubCommentSync.objects.update_or_create(issue_sync, comment, …)` —
/// keyed by `(issue_sync, comment)` (`unique_together`, `github.py:116`),
/// NOT by the remote id. `$1` the issue-sync id, `$2` the comment id.
pub const COMMENT_SYNC_LOOKUP_SQL: &str = "SELECT id FROM github_comment_syncs WHERE issue_sync_id = $1 AND comment_id = $2 AND deleted_at IS NULL";

/// `GithubCommentSync.objects.update_or_create(issue_sync, comment, …)`
/// (`github_sync_task.py:230-240`): no metadata column on this table;
/// audit lands NULL on both arms (no restamp — ported bug, see the module
/// docs).
const INSERT_COMMENT_SYNC_SQL: &str = "INSERT INTO github_comment_syncs (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, repo_comment_id, comment_id, issue_sync_id) VALUES ($1, $2, $3, NULL, NULL, NULL, $4, $5, $6, $7, $8)";
const UPDATE_COMMENT_SYNC_SQL: &str = "UPDATE github_comment_syncs SET repo_comment_id = $2, workspace_id = $3, project_id = $4, created_by_id = NULL, updated_by_id = NULL, updated_at = $5 WHERE id = $1";

/// Sync outcomes: success stamps both columns
/// (`github_sync_task.py:323-325`); faults write only `last_sync_error`
/// (`save(update_fields=[…])` — no `updated_at` touch either way).
const SYNC_SUCCESS_SQL: &str =
    "UPDATE github_repository_syncs SET last_synced_at = $2, last_sync_error = '' WHERE id = $1";
const SYNC_ERROR_SQL: &str =
    "UPDATE github_repository_syncs SET last_sync_error = $2 WHERE id = $1";

/// Metadata-only writes (`save(update_fields=["metadata"])`):
/// reconcile flags, the login merge, and the completion guard/error/id
/// keys. Django's `_save_table` writes exactly the named columns —
/// `auto_now` is not folded in — so no `updated_at` touch here either.
const UPDATE_GITHUB_SYNC_METADATA_SQL: &str =
    "UPDATE github_issue_syncs SET metadata = $2 WHERE id = $1";

/// Reconcile candidate rows (`github_sync_task.py:249`): id, number and
/// metadata of every live mirror on the sync. `$1` is the sync id.
pub const RECONCILE_LIST_SQL: &str =
    "SELECT id, repo_issue_id, metadata FROM github_issue_syncs WHERE repository_sync_id = $1 AND deleted_at IS NULL";

/// Completion closure (`github_sync_task.py:344-350`): the sync row with
/// its `select_related` (`repository_sync__repository`,
/// `repository_sync__workspace_integration`, `issue`,
/// `issue__workspace`) — plain inner joins, base row scoped. `$1` is the
/// issue-sync id; a miss is the `DoesNotExist` no-op.
pub const COMPLETION_LOOKUP_SQL: &str =
    "SELECT s.id AS sync_id, s.repo_issue_id AS sync_repo_issue_id, s.metadata AS sync_metadata, \
    rs.id AS repo_sync_id, rs.workspace_integration_id AS repo_sync_integration_id, \
    r.owner AS repo_owner, r.name AS repo_name, \
    wi.config AS integration_config, \
    i.id AS issue_id, i.project_id AS issue_project_id, \
    w.slug AS workspace_slug \
    FROM github_issue_syncs s \
    INNER JOIN github_repository_syncs rs ON rs.id = s.repository_sync_id \
    INNER JOIN github_repositories r ON r.id = rs.repository_id \
    INNER JOIN workspace_integrations wi ON wi.id = rs.workspace_integration_id \
    INNER JOIN issues i ON i.id = s.issue_id \
    INNER JOIN workspaces w ON w.id = i.workspace_id \
    WHERE s.id = $1 AND s.deleted_at IS NULL";

// ---------------------------------------------------------------------------
// Celery wire payloads
// ---------------------------------------------------------------------------

/// One fan-out job per enabled sync (`github_sync_task.py:271`):
/// `sync_one_repo.delay(str(sync_id))` is `args=[str]`, `kwargs={}` on
/// the wire. `max_retries` rides the row default
/// ([`DEFAULT_MAX_RETRIES`] == [`MAX_RETRIES`], asserted in tests).
pub fn fanout_job(sync_id: &Uuid) -> NewJob {
    NewJob::new(SYNC_ONE_REPO_TASK, json!([sync_id.to_string()]), json!({}))
}

/// The Celery v2 message a fan-out row becomes on the wire: same task
/// name, `args=[str(id)]`, empty kwargs — byte-shape identical to the
/// Python `.delay(str(id))` call. Repeats [`crate::worker::dispatch`]'s
/// forward-path mapping arm for arm (that function is the runtime source
/// of truth; this one exists so tests can assert the wire contract without
/// a broker, following the `tasks_ticker::scan::fire_message` precedent).
pub fn fanout_message(sync_id: &Uuid) -> CeleryTaskMessage {
    let job = fanout_job(sync_id);
    let args = match job.args {
        Value::Array(items) => items,
        other => vec![other],
    };
    let kwargs = match job.kwargs {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    CeleryTaskMessage::new(SYNC_ONE_REPO_TASK, args, kwargs)
}

/// One completion job (`post_completion_comment.delay(str(issue_sync.id))`,
/// `github_sync_task.py:74`, `github_signals.py:74`): same wire shape as
/// the fan-out — `args=[str]`, `kwargs={}`, first attempt, no ETA.
pub fn completion_job(sync_id: &Uuid) -> NewJob {
    NewJob::new(
        POST_COMPLETION_COMMENT_TASK,
        json!([sync_id.to_string()]),
        json!({}),
    )
}

// ---------------------------------------------------------------------------
// Live transport (the raw `GithubClient` seam, `github_client.py:38-220`)
// ---------------------------------------------------------------------------

/// Raw GitHub REST calls the scan needs (`GithubClient`,
/// `github_client.py:38-160`): the two paginated listings plus the
/// comment POST, each returning decoded JSON. Object-safe so handlers
/// take `Arc<dyn GithubSyncTransport>` and tests inject scripted fakes.
pub trait GithubSyncTransport: Send + Sync {
    /// Paginated `/repos/{owner}/{repo}/issues?state=open`
    /// (`github_client.py:133-140`): PRs arrive alongside issues; the
    /// scan filters them by `pull_request` key presence.
    fn list_all_open_issues(&self, owner: &str, name: &str) -> Result<Vec<Value>, GithubError>;

    /// Paginated repo-wide `/repos/{owner}/{repo}/issues/comments`
    /// (`github_client.py:141-147`): covers every issue and PR; the scan
    /// keeps only comments whose parent is a mirrored open issue.
    fn list_all_repo_comments(&self, owner: &str, name: &str) -> Result<Vec<Value>, GithubError>;

    /// `POST /repos/{owner}/{repo}/issues/{number}/comments`
    /// (`github_client.py:156-160`).
    fn post_issue_comment(
        &self,
        owner: &str,
        name: &str,
        issue_number: i64,
        body: &str,
    ) -> Result<Value, GithubError>;
}

/// Live GitHub REST transport (`GithubClient(token=…, api_base=…)`):
/// `Bearer` token auth, the `vnd.github+json` accept headers, exact status
/// mapping (401/403/404 → provider errors; anything else unsuccessful
/// surfaces as [`GithubError::Transport`], like `raise_for_status`'s
/// `HTTPError` passing through `_map_error` unchanged), and `Link
/// rel="next"` pagination (`_paginate`, `github_client.py:87-96`).
#[derive(Debug, Clone)]
pub struct GithubRestClient {
    http: reqwest::blocking::Client,
    token: String,
    api_base: String,
    timeout_secs: u64,
}

impl GithubRestClient {
    /// `GithubClient(token=…, timeout=…, api_base=…)`
    /// (`github_client.py:39-44`): empty tokens fail closed with
    /// `Auth("empty token")`; `api_base` keeps its trailing slash
    /// stripped, like Python's `rstrip("/")`.
    pub fn connect(token: &str, timeout_secs: u64, api_base: &str) -> Result<Self, GithubError> {
        if token.is_empty() {
            return Err(GithubError::Auth("empty token".to_owned()));
        }
        let http = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| GithubError::Transport(error.to_string()))?;
        Ok(Self {
            http,
            token: token.to_owned(),
            api_base: api_base.trim_end_matches('/').to_owned(),
            timeout_secs,
        })
    }

    /// The default construction (`GithubClient(token=token)`):
    /// 30-second timeout against `https://api.github.com`.
    pub fn live(token: &str) -> Result<Self, GithubError> {
        Self::connect(token, PROVIDER_TIMEOUT_SECS, GITHUB_API_BASE)
    }

    fn send(
        &self,
        method: reqwest::Method,
        url: &str,
        json_body: Option<&Value>,
    ) -> Result<reqwest::blocking::Response, GithubError> {
        let mut request = self
            .http
            .request(method, url)
            .timeout(Duration::from_secs(self.timeout_secs))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "pi-dash-github-sync");
        if let Some(body) = json_body {
            let text = serde_json::to_string(body)
                .map_err(|error| GithubError::Transport(error.to_string()))?;
            request = request
                .header("Content-Type", "application/json")
                .body(text);
        }
        request
            .send()
            .map_err(|error| GithubError::Transport(error.to_string()))
    }

    fn checked(
        &self,
        method: reqwest::Method,
        url: &str,
        json_body: Option<&Value>,
    ) -> Result<reqwest::blocking::Response, GithubError> {
        let response = self.send(method, url, json_body)?;
        let status = response.status().as_u16();
        // `_request` (`github_client.py:62-73`): exact mapping first,
        // `raise_for_status` for the rest.
        if status == 401 {
            return Err(GithubError::Auth(response_text(response)));
        }
        if status == 403 {
            return Err(GithubError::Permission(response_text(response)));
        }
        if status == 404 {
            return Err(GithubError::NotFound(response_text(response)));
        }
        if !response.status().is_success() {
            let body = response_text(response);
            return Err(GithubError::Transport(format!(
                "GitHub API request failed: HTTP {status}: {body}"
            )));
        }
        Ok(response)
    }

    /// `_paginate` (`github_client.py:87-96`): follow `rel="next"` until
    /// exhausted, concatenating every page's items.
    fn paginate(&self, path: &str, params: &[(&str, String)]) -> Result<Vec<Value>, GithubError> {
        let mut url = format!("{}{path}", self.api_base);
        if !params.is_empty() {
            let query = params
                .iter()
                .map(|(key, value)| format!("{key}={}", percent_encode(value)))
                .collect::<Vec<_>>()
                .join("&");
            url = format!("{url}?{query}");
        }
        let mut items = Vec::new();
        let mut next: Option<String> = Some(url);
        while let Some(current) = next {
            let response = self.checked(reqwest::Method::GET, &current, None)?;
            let link = response
                .headers()
                .get("link")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_owned();
            let page = response_json_list(response)?;
            items.extend(page);
            next = next_link(&link);
        }
        Ok(items)
    }
}

/// Read a response body for error text (`response.text`,
/// `github_client.py:67-71`). A decode failure degrades to `""` — the
/// status mapping above already decided the variant.
fn response_text(response: reqwest::blocking::Response) -> String {
    response.text().unwrap_or_default()
}

/// Decode a JSON response body (`response.json()`): undecodable bodies are
/// transport faults — in Python the decoder error propagates as a
/// non-provider error, the same observable class.
fn response_json(response: reqwest::blocking::Response) -> Result<Value, GithubError> {
    let text = response
        .text()
        .map_err(|error| GithubError::Transport(error.to_string()))?;
    serde_json::from_str(&text).map_err(|error| GithubError::Transport(error.to_string()))
}

/// Decode a JSON response body known to be a list (every paginated GitHub
/// endpoint returns one).
fn response_json_list(response: reqwest::blocking::Response) -> Result<Vec<Value>, GithubError> {
    response_json(response).and_then(|body| {
        body.as_array()
            .cloned()
            .ok_or_else(|| GithubError::Transport("GitHub API returned a non-list page".to_owned()))
    })
}

/// Minimal percent-encoding for query values (mirrors `urlencode` for the
/// ASCII parameter values used here: `open`, digits, `updated`/`desc`).
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

impl GithubSyncTransport for GithubRestClient {
    /// Paginated `/issues?state=open` (`github_client.py:133-140`).
    fn list_all_open_issues(&self, owner: &str, name: &str) -> Result<Vec<Value>, GithubError> {
        self.paginate(
            &format!("/repos/{owner}/{name}/issues"),
            &[
                ("state", "open".to_owned()),
                ("per_page", "100".to_owned()),
                ("sort", "updated".to_owned()),
                ("direction", "desc".to_owned()),
            ],
        )
    }

    /// Paginated repo-wide `/issues/comments`
    /// (`github_client.py:141-147`).
    fn list_all_repo_comments(&self, owner: &str, name: &str) -> Result<Vec<Value>, GithubError> {
        self.paginate(
            &format!("/repos/{owner}/{name}/issues/comments"),
            &[
                ("per_page", "100".to_owned()),
                ("sort", "updated".to_owned()),
                ("direction", "asc".to_owned()),
            ],
        )
    }

    /// `POST …/issues/{number}/comments` (`github_client.py:156-160`).
    fn post_issue_comment(
        &self,
        owner: &str,
        name: &str,
        issue_number: i64,
        body: &str,
    ) -> Result<Value, GithubError> {
        let response = self.checked(
            reqwest::Method::POST,
            &format!(
                "{}/repos/{owner}/{name}/issues/{issue_number}/comments",
                self.api_base
            ),
            Some(&json!({ "body": body })),
        )?;
        response_json(response)
    }
}

// ---------------------------------------------------------------------------
// Scan state + upserts (mirrors the ORM bodies statement-for-statement)
// ---------------------------------------------------------------------------

/// Scan-section fault (`github_sync_task.py:327-335`): provider 4xx faults
/// record `last_sync_error` and ack; anything else records and takes the
/// `self.retry(countdown)` path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanFault {
    Recordable(GithubError),
    Unexpected(String),
}

impl ScanFault {
    /// Classify a provider failure: auth/permission/not-found record;
    /// transport faults and every other error retry. (Python:
    /// `except (GithubAuthError, GithubPermissionError,
    /// GithubNotFoundError)` first, then `except Exception`.)
    pub fn classify(error: GithubError) -> Self {
        match error {
            inner @ (GithubError::Auth(_)
            | GithubError::Permission(_)
            | GithubError::NotFound(_)) => ScanFault::Recordable(inner),
            GithubError::Transport(message) => ScanFault::Unexpected(message),
        }
    }
}

/// Live dependencies for the scan handlers: the shared Fernet [`Keyring`]
/// (PAT decryption, like `decrypt_data` over `settings.SECRET_KEY`).
/// Constructed once at registration ([`LiveTransports::from_env`]) and
/// cloned into every handler (`Keyring` is `Clone`).
#[derive(Debug, Clone)]
pub struct LiveTransports {
    keyring: Keyring,
}

impl LiveTransports {
    pub fn from_env() -> Self {
        Self {
            keyring: Keyring::from_env(),
        }
    }

    pub fn with_keyring(keyring: Keyring) -> Self {
        Self { keyring }
    }

    /// `GithubClient(token=token)` (`github_sync_task.py:292`).
    pub fn client(&self, token: &str) -> Result<GithubRestClient, GithubError> {
        GithubRestClient::live(token)
    }
}

/// One sync's scan closure: the `select_related` row decoded
/// (`github_sync_task.py:280-282`) plus the decrypted PAT
/// (`_resolve_token`, `github_sync_task.py:286`).
pub struct RepoScan {
    pub sync_id: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub owner: String,
    pub name: String,
    pub token: String,
}

fn decode_sync_scan(row: sqlx::postgres::PgRow, keyring: &Keyring) -> Result<RepoScan, String> {
    let config: Value = row
        .try_get("integration_config")
        .map_err(|error| format!("decode sync scan: {error}"))?;
    let token =
        resolve_token(&config, keyring).map_err(|detail| format!("decode sync scan: {detail}"))?;
    Ok(RepoScan {
        sync_id: row
            .try_get("sync_id")
            .map_err(|error| format!("decode sync scan: {error}"))?,
        project_id: row
            .try_get("project_id")
            .map_err(|error| format!("decode sync scan: {error}"))?,
        workspace_id: row
            .try_get("workspace_id")
            .map_err(|error| format!("decode sync scan: {error}"))?,
        actor_id: row
            .try_get("actor_id")
            .map_err(|error| format!("decode sync scan: {error}"))?,
        owner: row
            .try_get("repo_owner")
            .map_err(|error| format!("decode sync scan: {error}"))?,
        name: row
            .try_get("repo_name")
            .map_err(|error| format!("decode sync scan: {error}"))?,
        token,
    })
}

/// Resolve the workspace for a new mirror issue (`ProjectBaseModel.save`,
/// `project.py:309-311`).
async fn project_workspace(pool: &PgPool, project_id: &Uuid) -> Result<Uuid, String> {
    sqlx::query_scalar(PROJECT_WORKSPACE_SQL)
        .bind(*project_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| format!("resolve project workspace: {error}"))?
        .ok_or_else(|| format!("resolve project workspace: project {project_id} is gone"))
}

/// Resolve the default state the way `Issue.save` does at creation
/// (`issue.py:288-301`): default non-triage state first, else first
/// non-triage state (`is_triage = FALSE`, `save_state_sql`).
async fn save_default_state(
    executor: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: &Uuid,
) -> Result<Option<Uuid>, String> {
    let db_error = |error: sqlx::Error| format!("create mirror issue: {error}");
    let state: Option<(Uuid, String)> = sqlx::query_as(save_state_sql(true))
        .bind(*project_id)
        .fetch_optional(&mut **executor)
        .await
        .map_err(db_error)?;
    if state.is_some() {
        return Ok(state.map(|found| found.0));
    }
    let fallback: Option<(Uuid, String)> = sqlx::query_as(save_state_sql(false))
        .bind(*project_id)
        .fetch_optional(&mut **executor)
        .await
        .map_err(db_error)?;
    Ok(fallback.map(|found| found.0))
}

/// Create one mirror issue plus its sync row (`_upsert_issue` create arm,
/// `github_sync_task.py:135-140`, with the `Issue.save()` creation path
/// `issue.py:267-340` inlined: workspace derivation, pod default,
/// non-triage default state, advisory-locked sequence, sort-order seed,
/// stripped recompute).
#[allow(clippy::too_many_arguments)]
async fn create_issue(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scan: &RepoScan,
    workspace_id: &Uuid,
    number: i64,
    name: &str,
    description_html: &str,
    // NOTE: the `_safe_render` stripped value is passed in but `save()`
    // overwrites it (`_ = default_state`'s sibling dead-input ported bug).
    _description_stripped: &str,
    sync_values: &IssueSyncValues,
    now: &DateTime<Utc>,
) -> Result<(Uuid, Uuid), String> {
    let db_error = |error: sqlx::Error| format!("create mirror issue: {error}");
    sqlx::query(ADVISORY_LOCK_SQL)
        .bind(advisory_lock_key(&scan.project_id))
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    let pod_id: Option<Uuid> = sqlx::query_scalar(DEFAULT_POD_SQL)
        .bind(scan.project_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_error)?;
    let state_id = save_default_state(tx, &scan.project_id).await?;
    // `completed_at` stays NULL here: this create call never passes a
    // state, so `Issue.save` takes the `state is None` branch
    // (`issue.py:288-296`) — it resolves the default state but stamps
    // `completed_at` only when the state was already set (`issue.py:302-309`,
    // which this path never reaches).
    let completed_at: Option<DateTime<Utc>> = None;
    let max_sequence: Option<i64> = sqlx::query_scalar(MAX_SEQUENCE_SQL)
        .bind(scan.project_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_error)?
        .flatten();
    let sequence_id = max_sequence.map(|largest| largest + 1).unwrap_or(1);
    let max_sort: Option<f64> = sqlx::query_scalar(MAX_SORT_ORDER_SQL)
        .bind(scan.project_id)
        .bind(state_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_error)?
        .flatten();
    let sort_order = max_sort.map(|largest| largest + 10000.0).unwrap_or(65535.0);
    // `save()` recomputes the stripped form (`issue.py:330-334`) with the
    // html_processor stripper, decoding entities.
    let stripped = strip_html_text(description_html);
    let issue_id = Uuid::new_v4();
    sqlx::query(INSERT_ISSUE_SQL)
        .bind(issue_id)
        .bind(*now)
        .bind(*now)
        .bind(scan.actor_id)
        .bind(scan.actor_id)
        .bind(scan.project_id)
        .bind(*workspace_id)
        .bind(state_id)
        .bind(name)
        .bind(json!({}))
        .bind(description_html)
        .bind(stripped)
        .bind(sequence_id)
        .bind(sort_order)
        .bind(completed_at)
        .bind("github")
        .bind(number.to_string())
        .bind(pod_id)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    sqlx::query(INSERT_ISSUE_SEQUENCE_SQL)
        .bind(Uuid::new_v4())
        .bind(*now)
        .bind(*now)
        .bind(scan.project_id)
        .bind(*workspace_id)
        .bind(issue_id)
        .bind(sequence_id)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    let sync_id =
        insert_issue_sync(tx, scan, workspace_id, &issue_id, number, sync_values, now).await?;
    Ok((issue_id, sync_id))
}

/// The `GithubIssueSync` defaults decoded once per upsert
/// (`github_sync_task.py:171-181`).
pub struct IssueSyncValues {
    pub github_issue_id: i64,
    pub issue_url: String,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

async fn insert_issue_sync(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scan: &RepoScan,
    workspace_id: &Uuid,
    issue_id: &Uuid,
    number: i64,
    values: &IssueSyncValues,
    now: &DateTime<Utc>,
) -> Result<Uuid, String> {
    let db_error = |error: sqlx::Error| format!("write issue sync: {error}");
    let sync_id = Uuid::new_v4();
    sqlx::query(INSERT_ISSUE_SYNC_SQL)
        .bind(sync_id)
        .bind(*now)
        .bind(*now)
        .bind(scan.project_id)
        .bind(*workspace_id)
        .bind(number)
        .bind(values.github_issue_id)
        .bind(&values.issue_url)
        .bind(*issue_id)
        .bind(scan.sync_id)
        .bind(values.created_at)
        .bind(values.updated_at)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    Ok(sync_id)
}

/// Decode one upstream issue payload into render + sync values
/// (`_upsert_issue`, `github_sync_task.py:121-124,171-181`). Missing
/// `number` raises (`gh_issue["number"]`, `KeyError` inside the guarded
/// `try` → record-and-retry); unparseable remote datetimes raise the same
/// way Django's `DateTimeField` validation would.
pub fn decode_issue_payload(
    gh_issue: &Value,
) -> Result<(i64, String, String, String, IssueSyncValues), String> {
    let number = gh_issue
        .get("number")
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("upstream issue has no integer number: {gh_issue}"))?;
    let title = coerce_title(gh_issue.get("title"));
    let name = github_issue_name(number, &title);
    let body = coerce_body(gh_issue.get("body"))
        .map_err(|detail| format!("upstream issue #{number}: {detail}"))?;
    let (description_html, description_stripped) = safe_render(body.as_deref());
    let values = IssueSyncValues {
        github_issue_id: gh_issue.get("id").and_then(Value::as_i64).unwrap_or(0),
        issue_url: gh_issue
            .get("html_url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        created_at: parse_remote_dt(gh_issue.get("created_at").and_then(Value::as_str))
            .map_err(|detail| format!("upstream issue #{number}: {detail}"))?,
        updated_at: parse_remote_dt(gh_issue.get("updated_at").and_then(Value::as_str))
            .map_err(|detail| format!("upstream issue #{number}: {detail}"))?,
    };
    Ok((number, name, description_html, description_stripped, values))
}

/// Create-or-update one local Issue mirror plus its GithubIssueSync row
/// (`_upsert_issue`, `github_sync_task.py:111-188`). Returns the local
/// `(issue_id, issue_sync_id)`.
pub async fn upsert_issue(
    pool: &PgPool,
    scan: &RepoScan,
    gh_issue: &Value,
    now: &DateTime<Utc>,
) -> Result<(Uuid, Uuid), String> {
    let db_error = |error: sqlx::Error| format!("upsert mirror issue: {error}");
    let (number, name, description_html, description_stripped, values) =
        decode_issue_payload(gh_issue)?;
    let number_text = number.to_string();
    let existing: Option<Uuid> = sqlx::query_scalar(ISSUE_LOOKUP_SQL)
        .bind(scan.project_id)
        .bind(&number_text)
        .fetch_optional(pool)
        .await
        .map_err(db_error)?;
    match existing {
        None => {
            let workspace_id = project_workspace(pool, &scan.project_id).await?;
            let mut tx = pool.begin().await.map_err(db_error)?;
            let ids = create_issue(
                &mut tx,
                scan,
                &workspace_id,
                number,
                &name,
                &description_html,
                &description_stripped,
                &values,
                now,
            )
            .await?;
            merge_issue_login(&mut tx, &ids.1, gh_issue).await?;
            tx.commit().await.map_err(db_error)?;
            Ok(ids)
        }
        Some(issue_id) => {
            // `setattr(defaults)` + full `Issue.save()`:
            // `completed_at` recomputes from the row's own state
            // (`issue.py:302-309`), the stripped form recomputes from the
            // new HTML, `updated_at` touches (auto_now) — then the audit
            // restamp via `filter(pk).update`, collapsed into the same
            // statement (identical final bytes).
            let (state_id, state_group, stored_completed_at): (
                Option<Uuid>,
                Option<String>,
                Option<DateTime<Utc>>,
            ) = sqlx::query_as(ISSUE_STATE_SQL)
                .bind(issue_id)
                .fetch_optional(pool)
                .await
                .map_err(db_error)?
                .unwrap_or((None, None, None));
            let completed_at: Option<DateTime<Utc>> = match (state_id, state_group.as_deref()) {
                (Some(_), Some("completed")) => Some(*now),
                (Some(_), _) => None,
                (None, _) => {
                    // `self.state is None` (`issue.py:288-296`): resolve
                    // the default and store it on the row. That branch
                    // never touches `completed_at`, so the stored stamp
                    // is preserved (it stays NULL on every row the sync
                    // itself created).
                    let mut tx = pool.begin().await.map_err(db_error)?;
                    let resolved = save_default_state(&mut tx, &scan.project_id).await?;
                    tx.commit().await.map_err(db_error)?;
                    sqlx::query(ADOPT_STATE_SQL)
                        .bind(issue_id)
                        .bind(resolved)
                        .execute(pool)
                        .await
                        .map_err(db_error)?;
                    stored_completed_at
                }
            };
            // `save()` recomputes the stripped form (`issue.py:345-350`).
            let stripped = strip_html_text(&description_html);
            sqlx::query(UPDATE_ISSUE_SQL)
                .bind(issue_id)
                .bind(&name)
                .bind(&description_html)
                .bind(&stripped)
                .bind(json!({}))
                .bind(scan.actor_id)
                .bind(completed_at)
                .bind(*now)
                .execute(pool)
                .await
                .map_err(db_error)?;
            let mut tx = pool.begin().await.map_err(db_error)?;
            let sync_id = update_issue_sync(&mut tx, scan, &issue_id, number, &values, now).await?;
            merge_issue_login(&mut tx, &sync_id, gh_issue).await?;
            tx.commit().await.map_err(db_error)?;
            Ok((issue_id, sync_id))
        }
    }
}

async fn update_issue_sync(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scan: &RepoScan,
    issue_id: &Uuid,
    number: i64,
    values: &IssueSyncValues,
    now: &DateTime<Utc>,
) -> Result<Uuid, String> {
    let db_error = |error: sqlx::Error| format!("write issue sync: {error}");
    let sync_id: Option<Uuid> = sqlx::query_scalar(GITHUB_SYNC_LOOKUP_SQL)
        .bind(scan.sync_id)
        .bind(*issue_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_error)?;
    match sync_id {
        None => {
            let workspace_id: Uuid = sqlx::query_scalar(PROJECT_WORKSPACE_SQL)
                .bind(scan.project_id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(db_error)?
                .ok_or_else(|| {
                    format!(
                        "resolve project workspace: project {} is gone",
                        scan.project_id
                    )
                })?;
            insert_issue_sync(tx, scan, &workspace_id, issue_id, number, values, now).await
        }
        Some(sync_id) => {
            sqlx::query(UPDATE_ISSUE_SYNC_SQL)
                .bind(sync_id)
                .bind(number)
                .bind(values.github_issue_id)
                .bind(&values.issue_url)
                .bind(scan.workspace_id)
                .bind(scan.project_id)
                .bind(values.created_at)
                .bind(values.updated_at)
                .bind(*now)
                .execute(&mut **tx)
                .await
                .map_err(db_error)?;
            Ok(sync_id)
        }
    }
}

/// `if user.get("login")` metadata merge (`github_sync_task.py:183-186`):
/// read-modify-write of the sync metadata, `save(update_fields=
/// ["metadata"])` — metadata only, no `updated_at` touch.
async fn merge_issue_login(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    sync_id: &Uuid,
    gh_issue: &Value,
) -> Result<(), String> {
    let login = gh_issue
        .get("user")
        .and_then(|user| user.get("login"))
        .and_then(Value::as_str)
        .filter(|login| !login.is_empty());
    let Some(login) = login else {
        return Ok(());
    };
    let db_error = |error: sqlx::Error| format!("merge issue login: {error}");
    let metadata: Value =
        sqlx::query_scalar("SELECT metadata FROM github_issue_syncs WHERE id = $1")
            .bind(*sync_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_error)?
            .unwrap_or(json!({}));
    // `metadata` is a JSONField (`default=dict`): objects merge in place;
    // a NULL cell (only reachable on a hand-built row) starts at `{}`.
    let mut metadata = match metadata {
        Value::Object(_) => metadata,
        _ => json!({}),
    };
    metadata[github_user_login_key()] = Value::String(login.to_owned());
    sqlx::query(UPDATE_GITHUB_SYNC_METADATA_SQL)
        .bind(*sync_id)
        .bind(metadata)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    Ok(())
}

fn github_user_login_key() -> &'static str {
    "github_user_login"
}

/// Create-or-update one mirrored comment (`_upsert_comment`,
/// `github_sync_task.py:191-240`) with the `IssueComment.save`
/// description side-table (`issue.py:598-646`).
pub async fn upsert_comment(
    pool: &PgPool,
    scan: &RepoScan,
    gh_comment: &Value,
    issue_id: &Uuid,
    sync_id: &Uuid,
    now: &DateTime<Utc>,
) -> Result<(), String> {
    let db_error = |error: sqlx::Error| format!("upsert mirror comment: {error}");
    let remote_id = gh_comment
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("upstream comment has no integer id: {gh_comment}"))?;
    let body = coerce_body(gh_comment.get("body"))
        .map_err(|detail| format!("upstream comment {remote_id}: {detail}"))?;
    let (safe_html, _safe_stripped) = safe_render(body.as_deref());
    let comment_html = mirror_comment_html(&safe_html);
    // See [`mirror_comment_stripped`]: `save()` recomputes the column, so
    // the trimmed intermediate never reaches the database.
    let comment_stripped = mirror_comment_stripped(&comment_html);
    let remote_id_text = remote_id.to_string();
    let existing: Option<(Uuid, String, String, Value, Option<Uuid>)> =
        sqlx::query_as(ISSUE_COMMENT_LOOKUP_SQL)
            .bind(*issue_id)
            .bind(&remote_id_text)
            .fetch_optional(pool)
            .await
            .map_err(db_error)?;
    let mut tx = pool.begin().await.map_err(db_error)?;
    let comment_id = match existing {
        None => {
            let description_id = Uuid::new_v4();
            sqlx::query(INSERT_DESCRIPTION_SQL)
                .bind(description_id)
                .bind(*now)
                .bind(*now)
                .bind(scan.actor_id)
                .bind(scan.actor_id)
                .bind(scan.workspace_id)
                .bind(scan.project_id)
                .bind(json!({}))
                .bind(&comment_html)
                .bind(strip_tags(&comment_html))
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
            let comment_id = Uuid::new_v4();
            sqlx::query(INSERT_ISSUE_COMMENT_SQL)
                .bind(comment_id)
                .bind(*now)
                .bind(*now)
                .bind(scan.actor_id)
                .bind(scan.actor_id)
                .bind(scan.project_id)
                .bind(scan.workspace_id)
                .bind(&comment_stripped)
                .bind(json!({}))
                .bind(&comment_html)
                .bind(description_id)
                .bind(*issue_id)
                .bind(scan.actor_id)
                .bind(&remote_id_text)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
            comment_id
        }
        Some((comment_id, old_html, old_stripped, old_json, description_id)) => {
            sqlx::query(UPDATE_ISSUE_COMMENT_SQL)
                .bind(comment_id)
                .bind(&comment_html)
                .bind(&comment_stripped)
                .bind(json!({}))
                .bind(scan.workspace_id)
                .bind(scan.project_id)
                .bind(scan.actor_id)
                .bind(scan.actor_id)
                .bind(*now)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
            // Change-tracked description refresh (`issue.py:629-646`):
            // only the render triple, only when it changed, only with a
            // description row to write to. The comparison runs against the
            // save()-recomputed values (decoded, unstripped), which is what
            // the database holds.
            let changed = old_html != comment_html
                || old_stripped != comment_stripped
                || old_json != json!({});
            if changed {
                if let Some(description_id) = description_id {
                    sqlx::query(UPDATE_COMMENT_DESCRIPTION_SQL)
                        .bind(description_id)
                        .bind(&comment_html)
                        .bind(&comment_stripped)
                        .bind(json!({}))
                        .bind(scan.actor_id)
                        .bind(*now)
                        .execute(&mut *tx)
                        .await
                        .map_err(db_error)?;
                }
            }
            comment_id
        }
    };
    let sync_row: Option<Uuid> = sqlx::query_scalar(COMMENT_SYNC_LOOKUP_SQL)
        .bind(*sync_id)
        .bind(comment_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?;
    match sync_row {
        None => {
            sqlx::query(INSERT_COMMENT_SYNC_SQL)
                .bind(Uuid::new_v4())
                .bind(*now)
                .bind(*now)
                .bind(scan.project_id)
                .bind(scan.workspace_id)
                .bind(remote_id)
                .bind(comment_id)
                .bind(*sync_id)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
        }
        Some(row_id) => {
            sqlx::query(UPDATE_COMMENT_SYNC_SQL)
                .bind(row_id)
                .bind(remote_id)
                .bind(scan.workspace_id)
                .bind(scan.project_id)
                .bind(*now)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
        }
    }
    tx.commit().await.map_err(db_error)?;
    Ok(())
}

/// Flag local mirrors absent from the remote listing
/// (`_reconcile_upstream_gone`, `github_sync_task.py:246-258`):
/// metadata-only writes, `upstream_gone_at` set to `now` in `isoformat`
/// shape ([`format_eta`] renders exactly that: explicit `+00:00`,
/// microseconds only when nonzero).
pub async fn reconcile_upstream_gone(
    pool: &PgPool,
    sync_id: &Uuid,
    remote_issue_numbers: &HashSet<i64>,
    now: &DateTime<Utc>,
) -> Result<(), String> {
    let db_error = |error: sqlx::Error| format!("reconcile mirrors: {error}");
    let rows: Vec<(Uuid, i64, Value)> = sqlx::query_as(RECONCILE_LIST_SQL)
        .bind(*sync_id)
        .fetch_all(pool)
        .await
        .map_err(db_error)?;
    for (row_id, repo_issue_id, metadata) in rows {
        let mut metadata = metadata;
        let is_present = remote_issue_numbers.contains(&repo_issue_id);
        let was_flagged = metadata.get("upstream_gone_at").is_some_and(json_truthy);
        if !is_present && !was_flagged {
            metadata["upstream_gone_at"] = Value::String(format_eta(*now));
            sqlx::query(UPDATE_GITHUB_SYNC_METADATA_SQL)
                .bind(row_id)
                .bind(metadata)
                .execute(pool)
                .await
                .map_err(db_error)?;
        } else if is_present && was_flagged {
            if let Value::Object(ref mut map) = metadata {
                map.remove("upstream_gone_at");
            }
            sqlx::query(UPDATE_GITHUB_SYNC_METADATA_SQL)
                .bind(row_id)
                .bind(metadata)
                .execute(pool)
                .await
                .map_err(db_error)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Task entry points
// ---------------------------------------------------------------------------

/// `sync_all_repos` (`github_sync_task.py:264-271`): beat-driven fan-out
/// — one queue row per enabled sync. Returns the fan-out count (Python
/// returns `None`; the count is the observable parity surface the oracle
/// asserts). A database failure parks as `Fail`: like Celery's task
/// failure it never retries, and the next beat tick refires anyway.
pub async fn sync_all_repos(pool: &PgPool) -> Result<usize, String> {
    if !github_sync_enabled() {
        return Ok(0);
    }
    let ids: Vec<Uuid> = sqlx::query_scalar(ENABLED_SYNCS_SQL)
        .fetch_all(pool)
        .await
        .map_err(|error| format!("{SYNC_ALL_REPOS_TASK}: enabled scan failed: {error}"))?;
    for id in &ids {
        enqueue(pool, &fanout_job(id))
            .await
            .map_err(|error| format!("{SYNC_ALL_REPOS_TASK}: fan-out failed: {error}"))?;
    }
    Ok(ids.len())
}

/// Full-scan sync of one repo (`sync_one_repo`,
/// `github_sync_task.py:274-335`): unknown ids and disabled switches ack
/// silently; missing credentials and provider 4xx faults record and ack;
/// unexpected faults record and retry with
/// [`retry_countdown_secs`]; everything before the guarded region (bad
/// ids, missing rows, undecryptable configs, setup-query failures) parks
/// as `Fail`, like Celery's unhandled task failure.
pub async fn sync_one_repo(
    pool: &PgPool,
    transports: &LiveTransports,
    sync_id: &str,
    attempts: u32,
) -> Result<Verdict, HandlerError> {
    if !github_sync_enabled() {
        return Ok(Verdict::Ack);
    }
    // `…objects.get(id=sync_id)`: garbage ids raise `ValidationError`
    // (task failure, no retry); misses raise `DoesNotExist` (silent ack).
    let id = match Uuid::parse_str(sync_id) {
        Ok(id) => id,
        Err(_) => {
            return Ok(Verdict::Fail {
                error: format!("{SYNC_ONE_REPO_TASK}: invalid sync id {sync_id:?}"),
            });
        }
    };
    let row = sqlx::query(SYNC_SCAN_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|error| format!("{SYNC_ONE_REPO_TASK}: sync lookup failed: {error}"))?;
    let Some(row) = row else {
        return Ok(Verdict::Ack);
    };
    let scan = decode_sync_scan(row, &transports.keyring).map_err(|detail| {
        // `_resolve_token` runs outside the guarded `try`
        // (`github_sync_task.py:286`): a non-string token fails the task
        // without recording.
        format!("{SYNC_ONE_REPO_TASK}: {detail}")
    })?;
    if scan.token.is_empty() {
        // `decrypt_data` fails closed to `""`, which takes the
        // missing-credential path (`github_sync_task.py:287-290`).
        if let Err(error) = sqlx::query(SYNC_ERROR_SQL)
            .bind(scan.sync_id)
            .bind(MISSING_TOKEN_ERROR)
            .execute(pool)
            .await
        {
            return Ok(Verdict::Fail {
                error: format!("{SYNC_ONE_REPO_TASK}: fault record failed: {error}"),
            });
        }
        return Ok(Verdict::Ack);
    }
    // `_project_default_state` (kept, like Python, even though the create
    // path re-derives state in `save()` — the `_ = default_state` port).
    let default_state: Option<Uuid> = match sqlx::query_scalar(default_state_sql(true))
        .bind(scan.project_id)
        .fetch_optional(pool)
        .await
    {
        Ok(found) => found,
        Err(error) => {
            return Ok(Verdict::Fail {
                error: format!("{SYNC_ONE_REPO_TASK}: default state lookup failed: {error}"),
            });
        }
    };
    let _default_state: Option<Uuid> = match default_state {
        Some(found) => Some(found),
        None => match sqlx::query_scalar(default_state_sql(false))
            .bind(scan.project_id)
            .fetch_optional(pool)
            .await
        {
            Ok(found) => found,
            Err(error) => {
                return Ok(Verdict::Fail {
                    error: format!("{SYNC_ONE_REPO_TASK}: default state lookup failed: {error}"),
                });
            }
        },
    };
    let transport = match transports.client(&scan.token) {
        Ok(client) => client,
        Err(error) => {
            // `GithubClient(token="")` cannot happen (empty tokens took
            // the missing-credential path above); a build failure is a
            // setup fault, like any error outside the guarded `try`.
            return Ok(Verdict::Fail {
                error: format!(
                    "{SYNC_ONE_REPO_TASK}: client build failed: {}",
                    error.message()
                ),
            });
        }
    };
    let now = Utc::now();
    match run_scan(pool, &transport, &scan, &now).await {
        Ok(()) => {
            if let Err(error) = sqlx::query(SYNC_SUCCESS_SQL)
                .bind(scan.sync_id)
                .bind(now)
                .execute(pool)
                .await
            {
                return Ok(Verdict::Fail {
                    error: format!("{SYNC_ONE_REPO_TASK}: success stamp failed: {error}"),
                });
            }
            Ok(Verdict::Ack)
        }
        Err(ScanFault::Recordable(error)) => {
            let text = scan_error_text(&error);
            if let Err(error) = sqlx::query(SYNC_ERROR_SQL)
                .bind(scan.sync_id)
                .bind(&text)
                .execute(pool)
                .await
            {
                return Ok(Verdict::Fail {
                    error: format!("{SYNC_ONE_REPO_TASK}: fault record failed: {error}"),
                });
            }
            Ok(Verdict::Ack)
        }
        Err(ScanFault::Unexpected(message)) => {
            let text = truncate_chars(&message, 1000);
            if let Err(error) = sqlx::query(SYNC_ERROR_SQL)
                .bind(scan.sync_id)
                .bind(&text)
                .execute(pool)
                .await
            {
                return Ok(Verdict::Fail {
                    error: format!("{SYNC_ONE_REPO_TASK}: fault record failed: {error}"),
                });
            }
            Ok(Verdict::Retry {
                delay_secs: retry_countdown_secs(attempts),
            })
        }
    }
}

/// The guarded scan body (`github_sync_task.py:298-325`): list open
/// issues (skipping PRs), upsert mirrors, enumerate repo-wide comments
/// (keeping only comments whose parent is a mirrored open issue), then
/// reconcile. Provider faults classify into [`ScanFault`]; database and
/// payload faults are unexpected (record + retry, like `except
/// Exception`).
async fn run_scan(
    pool: &PgPool,
    transport: &GithubRestClient,
    scan: &RepoScan,
    now: &DateTime<Utc>,
) -> Result<(), ScanFault> {
    let remote_issues = {
        let (owner, name) = (scan.owner.clone(), scan.name.clone());
        let transport = transport.clone();
        tokio::task::spawn_blocking(move || transport.list_all_open_issues(&owner, &name))
            .await
            .map_err(|error| ScanFault::Unexpected(format!("provider call failed: {error}")))?
            .map_err(ScanFault::classify)?
    };
    let mut remote_issue_numbers = HashSet::new();
    let mut pairs: Vec<(i64, Uuid, Uuid)> = Vec::new();
    for gh_issue in &remote_issues {
        // PRs arrive alongside issues (`github_sync_task.py:301-302`):
        // key presence skips, whatever the value.
        if gh_issue.get("pull_request").is_some() {
            continue;
        }
        let (issue_id, sync_id) = upsert_issue(pool, scan, gh_issue, now)
            .await
            .map_err(ScanFault::Unexpected)?;
        let number = gh_issue
            .get("number")
            .and_then(Value::as_i64)
            .ok_or_else(|| ScanFault::Unexpected("upstream issue lost its number".to_owned()))?;
        remote_issue_numbers.insert(number);
        pairs.push((number, issue_id, sync_id));
    }
    let remote_comments = {
        let (owner, name) = (scan.owner.clone(), scan.name.clone());
        let transport = transport.clone();
        tokio::task::spawn_blocking(move || transport.list_all_repo_comments(&owner, &name))
            .await
            .map_err(|error| ScanFault::Unexpected(format!("provider call failed: {error}")))?
            .map_err(ScanFault::classify)?
    };
    // Repo-wide enumeration (`github_sync_task.py:310-318`): the parent
    // must be in this scan's remote set AND have a local pair, else the
    // comment is skipped (PR/closed-issue/orphan comments).
    let mut by_number: std::collections::HashMap<i64, (Uuid, Uuid)> =
        std::collections::HashMap::new();
    for (number, issue_id, sync_id) in pairs {
        by_number.insert(number, (issue_id, sync_id));
    }
    for gh_comment in &remote_comments {
        let parent_number =
            parse_issue_number_from_url(gh_comment.get("issue_url").and_then(Value::as_str));
        let Some(parent_number) = parent_number else {
            continue;
        };
        if !remote_issue_numbers.contains(&parent_number) {
            continue;
        }
        let Some((issue_id, sync_id)) = by_number.get(&parent_number) else {
            continue;
        };
        upsert_comment(pool, scan, gh_comment, issue_id, sync_id, now)
            .await
            .map_err(ScanFault::Unexpected)?;
    }
    reconcile_upstream_gone(pool, &scan.sync_id, &remote_issue_numbers, now)
        .await
        .map_err(ScanFault::Unexpected)?;
    Ok(())
}

/// One-shot completion comment on the upstream GitHub issue
/// (`post_completion_comment`, `github_sync_task.py:338-389`): never
/// retries — every fault is recorded in the sync metadata and acked; only
/// malformed deliveries, a missing base URL and record failures park.
/// Unknown ids ack silently; already-posted mirrors short-circuit on the
/// `completion_comment_id` guard.
pub async fn post_completion_comment(
    pool: &PgPool,
    transports: &LiveTransports,
    sync_id: &str,
) -> Result<Verdict, HandlerError> {
    if !github_sync_enabled() {
        return Ok(Verdict::Ack);
    }
    let id = match Uuid::parse_str(sync_id) {
        Ok(id) => id,
        Err(_) => {
            return Ok(Verdict::Fail {
                error: format!("{POST_COMPLETION_COMMENT_TASK}: invalid issue sync id {sync_id:?}"),
            });
        }
    };
    let row = sqlx::query(COMPLETION_LOOKUP_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: sync lookup failed: {error}"))?;
    let Some(row) = row else {
        return Ok(Verdict::Ack);
    };
    let metadata: Value = row
        .try_get("sync_metadata")
        .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?;
    if metadata
        .get("completion_comment_id")
        .is_some_and(json_truthy)
    {
        return Ok(Verdict::Ack);
    }
    let config: Value = row
        .try_get("integration_config")
        .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?;
    let token = resolve_token(&config, &transports.keyring).map_err(|detail| {
        format!("{POST_COMPLETION_COMMENT_TASK}: token decode failed: {detail}")
    })?;
    if token.is_empty() {
        let mut metadata = metadata;
        metadata["completion_comment_error"] =
            Value::String(COMPLETION_MISSING_TOKEN_ERROR.to_owned());
        if let Err(error) = sqlx::query(UPDATE_GITHUB_SYNC_METADATA_SQL)
            .bind(id)
            .bind(metadata)
            .execute(pool)
            .await
        {
            return Ok(Verdict::Fail {
                error: format!("{POST_COMPLETION_COMMENT_TASK}: metadata record failed: {error}"),
            });
        }
        return Ok(Verdict::Ack);
    }
    // `WEB_URL or APP_BASE_URL` (`github_sync_task.py:101`): absent means
    // `ImproperlyConfigured` — a task failure, like any setup fault here.
    let base = completion_base_url(
        std::env::var("WEB_URL").ok().as_deref(),
        std::env::var("APP_BASE_URL").ok().as_deref(),
    );
    let Some(base) = base else {
        return Ok(Verdict::Fail {
            error: format!(
                "{POST_COMPLETION_COMMENT_TASK}: WEB_URL or APP_BASE_URL must be set for GitHub completion comments"
            ),
        });
    };
    let issue_id: Uuid = row
        .try_get("issue_id")
        .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?;
    let project_id: Uuid = row
        .try_get("issue_project_id")
        .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?;
    let workspace_slug: String = row
        .try_get("workspace_slug")
        .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?;
    let repo_issue_id: i64 = row
        .try_get("sync_repo_issue_id")
        .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?;
    let owner: String = row
        .try_get("repo_owner")
        .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?;
    let name: String = row
        .try_get("repo_name")
        .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?;
    let body = completion_body(&pidash_issue_url(
        &base,
        &workspace_slug,
        &project_id,
        &issue_id,
    ));
    let transport = match transports.client(&token) {
        Ok(client) => client,
        Err(error) => {
            return Ok(Verdict::Fail {
                error: format!(
                    "{POST_COMPLETION_COMMENT_TASK}: client build failed: {}",
                    error.message()
                ),
            });
        }
    };
    let posted = {
        let transport = transport.clone();
        tokio::task::spawn_blocking(move || {
            transport.post_issue_comment(&owner, &name, repo_issue_id, &body)
        })
        .await
        .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: provider call failed: {error}"))?
    };
    let mut metadata = metadata;
    match posted {
        Ok(comment) => {
            // Success stores the id and clears any earlier error
            // (`github_sync_task.py:387-389`). `comment.get("id")` keeps
            // whatever JSON shape GitHub returned (a number in practice).
            metadata["completion_comment_id"] = comment.get("id").cloned().unwrap_or(Value::Null);
            if let Value::Object(ref mut map) = metadata {
                map.remove("completion_comment_error");
            }
        }
        Err(error) => {
            // 4xx faults record `Type: message`; anything else records the
            // bare message (`github_sync_task.py:377-384`). Either way the
            // task returns — no retry, even for transport faults.
            let text = match &error {
                GithubError::Transport(message) => truncate_chars(message, 500),
                other => completion_error_text(scan_error_label(other), other.message()),
            };
            metadata["completion_comment_error"] = Value::String(text);
        }
    }
    if let Err(error) = sqlx::query(UPDATE_GITHUB_SYNC_METADATA_SQL)
        .bind(id)
        .bind(metadata)
        .execute(pool)
        .await
    {
        return Ok(Verdict::Fail {
            error: format!("{POST_COMPLETION_COMMENT_TASK}: metadata record failed: {error}"),
        });
    }
    Ok(Verdict::Ack)
}

/// Register the three local handlers. The pool is captured by the
/// closures because [`Handler`][crate::worker::Handler] receives only the
/// claimed row; Celery arity is enforced by [`parse_single_id_arg`]
/// (malformed deliveries ack with a warning, like Celery discarding
/// invalid signatures). `sync_one_repo` spends the row's retry budget
/// with [`retry_countdown_secs`]; the other two never retry.
pub fn register_github_sync_tasks(
    registry: &mut Registry,
    pool: PgPool,
    transports: LiveTransports,
) {
    let scan_pool = pool.clone();
    registry.register(
        SYNC_ALL_REPOS_TASK,
        Arc::new(move |_job: JobRow| {
            let pool = scan_pool.clone();
            Box::pin(async move {
                match sync_all_repos(&pool).await {
                    Ok(count) => {
                        if count > 0 {
                            tracing::info!(
                                count,
                                task = SYNC_ALL_REPOS_TASK,
                                "github sync: dispatched repos"
                            );
                        }
                        Ok(Verdict::Ack)
                    }
                    Err(error) => Ok(Verdict::Fail { error }),
                }
            }) as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>
        }),
    );
    let one_pool = pool.clone();
    let one_transports = transports.clone();
    registry.register(
        SYNC_ONE_REPO_TASK,
        Arc::new(move |job: JobRow| {
            let pool = one_pool.clone();
            let transports = one_transports.clone();
            Box::pin(async move {
                let id = match parse_single_id_arg(SYNC_ONE_REPO_TASK, &job.args, &job.kwargs) {
                    Ok(id) => id,
                    Err(detail) => {
                        tracing::warn!("{detail}");
                        return Ok(Verdict::Ack);
                    }
                };
                let attempts = job.attempts.max(0) as u32;
                sync_one_repo(&pool, &transports, &id, attempts).await
            }) as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>
        }),
    );
    let comment_transports = transports;
    registry.register(
        POST_COMPLETION_COMMENT_TASK,
        Arc::new(move |job: JobRow| {
            let pool = pool.clone();
            let transports = comment_transports.clone();
            Box::pin(async move {
                let id =
                    match parse_single_id_arg(POST_COMPLETION_COMMENT_TASK, &job.args, &job.kwargs)
                    {
                        Ok(id) => id,
                        Err(detail) => {
                            tracing::warn!("{detail}");
                            return Ok(Verdict::Ack);
                        }
                    };
                post_completion_comment(&pool, &transports, &id).await
            }) as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::DEFAULT_MAX_RETRIES;
    use crate::worker::{route_for, Route};
    use serde_json::json;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/integrations")
    }

    fn fixture(name: &str) -> Value {
        let text = std::fs::read_to_string(fixtures_dir().join(name))
            .unwrap_or_else(|e| panic!("read fixture {name}: {e}"));
        serde_json::from_str(&text).expect("fixture is valid JSON")
    }

    fn parse(sql: &str) -> sqlparser::ast::Statement {
        let mut stmts =
            sqlparser::parser::Parser::parse_sql(&sqlparser::dialect::PostgreSqlDialect {}, sql)
                .unwrap_or_else(|e| panic!("SQL parses: {e}\n{sql}"));
        assert_eq!(stmts.len(), 1);
        stmts.pop().unwrap()
    }

    fn is_select(statement: &sqlparser::ast::Statement) -> bool {
        matches!(statement, sqlparser::ast::Statement::Query(_))
    }

    // Task identity: the three Celery names, in Python definition order.
    #[test]
    fn task_names_match_python() {
        assert_eq!(
            TASK_NAMES,
            [
                "pi_dash.bgtasks.github_sync_task.sync_all_repos",
                "pi_dash.bgtasks.github_sync_task.sync_one_repo",
                "pi_dash.bgtasks.github_sync_task.post_completion_comment",
            ]
        );
        assert_eq!(SYNC_ALL_REPOS_TASK, TASK_NAMES[0]);
        assert_eq!(SYNC_ONE_REPO_TASK, TASK_NAMES[1]);
        assert_eq!(POST_COMPLETION_COMMENT_TASK, TASK_NAMES[2]);
    }

    // Retry budget: `bind=True, max_retries=3` rides the row default.
    #[test]
    fn retry_budget_is_three() {
        assert_eq!(MAX_RETRIES, 3);
        assert_eq!(DEFAULT_MAX_RETRIES, 3);
        assert_eq!(fanout_job(&Uuid::nil()).max_retries, 3);
        assert_eq!(completion_job(&Uuid::nil()).max_retries, 3);
    }

    // Retry/ETA parity: `countdown=60 * 2**retries`, asserted per attempt.
    #[test]
    fn retry_countdown_matches_celery() {
        assert_eq!([0, 1, 2, 3].map(retry_countdown_secs), [60, 120, 240, 480]);
        // A corrupt row saturates instead of overflowing.
        assert!(retry_countdown_secs(u32::MAX) < u64::MAX);
    }

    // Kill switch: exactly `GITHUB_SYNC_ENABLED`, default on. There is no
    // `GIT_SYNC_ENABLED` first lookup on this path
    // (`github_sync_task.py:48-50`). One test owns the process env for the
    // variable (no other test in this process touches it).
    #[test]
    fn kill_switch_reads_github_flag_only() {
        let saved = std::env::var("GITHUB_SYNC_ENABLED").ok();
        struct Restore(Option<String>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => std::env::set_var("GITHUB_SYNC_ENABLED", value),
                    None => std::env::remove_var("GITHUB_SYNC_ENABLED"),
                }
            }
        }
        let _restore = Restore(saved);
        std::env::remove_var("GITHUB_SYNC_ENABLED");
        assert!(github_sync_enabled());
        std::env::set_var("GITHUB_SYNC_ENABLED", "false");
        assert!(!github_sync_enabled());
        std::env::set_var("GITHUB_SYNC_ENABLED", "TRUE");
        assert!(github_sync_enabled());
        std::env::set_var("GITHUB_SYNC_ENABLED", "yes");
        assert!(!github_sync_enabled());
    }

    // Mirror titles carry the `[github_<n>]` prefix, capped at 255 chars
    // (code points, never split bytes).
    #[test]
    fn github_name_prefix_truncates_by_chars() {
        assert_eq!(
            github_issue_name(7, "Upstream title"),
            "[github_7] Upstream title"
        );
        assert_eq!(github_issue_name(0, ""), "[github_0] ");
        let long = format!("{}tail", "é".repeat(300));
        let capped = github_issue_name(42, &long);
        assert_eq!(capped.chars().count(), 255);
        assert!(capped.starts_with("[github_42] "));
    }

    // `gh_issue.get("title") or ""` coercion.
    #[test]
    fn title_coercion_matches_python_or() {
        assert_eq!(coerce_title(None), "");
        assert_eq!(coerce_title(Some(&Value::Null)), "");
        assert_eq!(coerce_title(Some(&json!(""))), "");
        assert_eq!(coerce_title(Some(&json!("Hi"))), "Hi");
        assert_eq!(coerce_title(Some(&json!(0))), "");
        assert_eq!(coerce_title(Some(&json!(5))), "5");
        assert_eq!(coerce_title(Some(&json!(true))), "True");
        assert_eq!(coerce_title(Some(&json!(false))), "");
    }

    // Bodies: missing/null is None; strings pass; anything else errors
    // (Python raises `AttributeError` inside the guarded try).
    #[test]
    fn body_coercion_matches_python() {
        assert_eq!(coerce_body(None), Ok(None));
        assert_eq!(coerce_body(Some(&Value::Null)), Ok(None));
        assert_eq!(
            coerce_body(Some(&json!("a\n\nb"))),
            Ok(Some("a\n\nb".to_owned()))
        );
        assert!(coerce_body(Some(&json!(5))).is_err());
        assert!(coerce_body(Some(&json!(true))).is_err());
    }

    // Comment render: leading-paragraph prefix; the stored stripped form
    // is the save() recompute (the `.strip()` intermediate never lands).
    #[test]
    fn comment_render_matches_python() {
        let (safe_html, _) = safe_render(Some("hello"));
        let html = mirror_comment_html(&safe_html);
        assert_eq!(html, "<p>[Github] </p><p>hello</p>");
        assert_eq!(mirror_comment_stripped(&html), "[Github] hello");
        // Empty upstream bodies keep the trailing space after recompute
        // (the dead `.strip()` ported bug).
        let (empty_html, _) = safe_render(None);
        let empty = mirror_comment_html(&empty_html);
        assert_eq!(empty, "<p>[Github] </p><p></p>");
        assert_eq!(mirror_comment_stripped(&empty), "[Github] ");
    }

    // `parse_issue_number_from_url` vectors, generated from CPython's
    // `re` (`github_client.py:220-228`).
    #[test]
    fn issue_number_parsing_matches_python_regex() {
        for (input, expected) in [
            (None, None),
            (Some(""), None),
            (Some("https://api.github.com/repos/o/r/issues/12"), Some(12)),
            (Some("https://api.github.com/repos/o/r/issues/12/"), None),
            (Some("/repos/o/r/issues/0"), Some(0)),
            (Some("/repos/o/r/issues/007"), Some(7)),
            (Some("/repos/o/r/issues/"), None),
            (Some("/repos/o/r/issues/abc"), None),
            (Some("/repos/o/r/pulls/12"), None),
            (Some("/repos/o//issues/12"), None),
            (Some("/repos/o/r/issues/12x"), None),
            (Some("x/repos/a/b/issues/99"), Some(99)),
            (Some("/repos/a/b/c/issues/5"), None),
            (Some("/REPOS/o/r/issues/12"), None),
        ] {
            assert_eq!(parse_issue_number_from_url(input), expected, "{input:?}");
        }
    }

    // `_resolve_token`: every falsy shape yields `""` (the callers
    // treat it as missing — Python's `or ""`); round-trips through the
    // Fernet keyring; truthy non-string tokens error (Python raises
    // outside the guarded try).
    #[test]
    fn token_resolution_matches_python() {
        let keyring = pidash_db::config::encryption::Keyring::from_secret("test-secret-key");
        assert_eq!(resolve_token(&json!({}), &keyring), Ok(String::new()));
        assert_eq!(resolve_token(&json!(null), &keyring), Ok(String::new()));
        assert_eq!(
            resolve_token(&json!({"token": ""}), &keyring),
            Ok(String::new())
        );
        assert_eq!(
            resolve_token(&json!({"token": null}), &keyring),
            Ok(String::new())
        );
        // Falsy non-strings collapse through `or ""`, like Python.
        for falsy in [json!(false), json!(0), json!([]), json!({})] {
            assert_eq!(
                resolve_token(&json!({"token": falsy}), &keyring),
                Ok(String::new()),
                "{falsy}"
            );
        }
        let ciphertext = keyring.encrypt("pat-123");
        assert_eq!(
            resolve_token(&json!({"token": ciphertext}), &keyring),
            Ok("pat-123".to_owned())
        );
        // Undecryptable ciphertext fails closed to `""` (missing path).
        assert_eq!(
            resolve_token(&json!({"token": "not-a-token"}), &keyring),
            Ok(String::new())
        );
        assert!(resolve_token(&json!({"token": 5}), &keyring).is_err());
    }

    // Fault texts: `Github{Auth,Permission,NotFound}Error: msg` capped at
    // 900 chars; unexpected faults cap at 1000; completion errors at 500.
    #[test]
    fn error_texts_match_python_shapes() {
        assert_eq!(
            scan_error_text(&GithubError::Auth("bad".into())),
            "GithubAuthError: bad"
        );
        assert_eq!(
            scan_error_text(&GithubError::Permission("no".into())),
            "GithubPermissionError: no"
        );
        assert_eq!(
            scan_error_text(&GithubError::NotFound("gone".into())),
            "GithubNotFoundError: gone"
        );
        assert_eq!(
            scan_error_text(&GithubError::Transport("boom".into())),
            "GithubError: boom"
        );
        assert_eq!(truncate_chars(&"é".repeat(1000), 900).chars().count(), 900);
        // Message-only truncation (`github_sync_task.py:329`):
        // a 1000-char message keeps all 900 chars after the label.
        assert_eq!(
            scan_error_text(&GithubError::Auth("x".repeat(1000))),
            format!("GithubAuthError: {}", "x".repeat(900))
        );
        assert_eq!(
            scan_error_text(&GithubError::NotFound("é".repeat(1000))),
            format!("GithubNotFoundError: {}", "é".repeat(900))
        );
        assert_eq!(
            completion_error_text("GithubAuthError", &"y".repeat(600)),
            format!("GithubAuthError: {}", "y".repeat(500))
        );
        assert_eq!(
            completion_error_text("GithubPermissionError", &"é".repeat(600)),
            format!("GithubPermissionError: {}", "é".repeat(500))
        );
        assert_eq!(
            completion_error_text("GithubAuthError", "denied"),
            "GithubAuthError: denied"
        );
        assert_eq!(truncate_chars(&"é".repeat(600), 500).chars().count(), 500);
    }

    // Fault classification: 4xx records, everything else retries.
    #[test]
    fn scan_fault_classification_matches_except_order() {
        assert_eq!(
            ScanFault::classify(GithubError::Auth("bad token".into())),
            ScanFault::Recordable(GithubError::Auth("bad token".into()))
        );
        assert_eq!(
            ScanFault::classify(GithubError::Permission("denied".into())),
            ScanFault::Recordable(GithubError::Permission("denied".into()))
        );
        assert_eq!(
            ScanFault::classify(GithubError::NotFound("gone".into())),
            ScanFault::Recordable(GithubError::NotFound("gone".into()))
        );
        assert_eq!(
            ScanFault::classify(GithubError::Transport("boom".into())),
            ScanFault::Unexpected("boom".into())
        );
    }

    // Fan-out + completion wire: `.delay(str(id))` is `args=[str]`,
    // `kwargs={}`, first attempt, no ETA.
    #[test]
    fn fanout_wire_matches_delay() {
        let id = Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap();
        let job = fanout_job(&id);
        assert_eq!(job.task, SYNC_ONE_REPO_TASK);
        assert_eq!(job.args, json!(["12345678-1234-5678-1234-567812345678"]));
        assert_eq!(job.kwargs, json!({}));
        let message = fanout_message(&id);
        assert_eq!(message.task, SYNC_ONE_REPO_TASK);
        assert_eq!(
            message.args,
            vec![json!("12345678-1234-5678-1234-567812345678")]
        );
        assert!(message.kwargs.is_empty());
        assert_eq!(message.retries, 0);
        assert!(message.eta.is_none());
        let body = message.body();
        assert_eq!(body[0], json!(["12345678-1234-5678-1234-567812345678"]));
        assert_eq!(body[1], json!({}));
        let headers = message.headers();
        assert_eq!(headers["task"], SYNC_ONE_REPO_TASK);
        assert_eq!(headers["lang"], "py");

        let comment = completion_job(&id);
        assert_eq!(comment.task, POST_COMPLETION_COMMENT_TASK);
        assert_eq!(
            comment.args,
            json!(["12345678-1234-5678-1234-567812345678"])
        );
        assert_eq!(comment.kwargs, json!({}));
    }

    // Arity: exactly `args=[<id>]`, `kwargs={}`; anything else is a
    // malformed delivery (ack-and-warn, never retry).
    #[test]
    fn single_id_arity_matches_celery() {
        let kwargs = json!({});
        assert_eq!(
            parse_single_id_arg(SYNC_ONE_REPO_TASK, &json!(["abc"]), &kwargs),
            Ok("abc".to_owned())
        );
        assert_eq!(
            parse_single_id_arg(POST_COMPLETION_COMMENT_TASK, &json!(["abc"]), &kwargs),
            Ok("abc".to_owned())
        );
        assert!(parse_single_id_arg(SYNC_ONE_REPO_TASK, &json!([]), &kwargs).is_err());
        assert!(parse_single_id_arg(SYNC_ONE_REPO_TASK, &json!(["a", "b"]), &kwargs).is_err());
        assert!(parse_single_id_arg(SYNC_ONE_REPO_TASK, &json!([1]), &kwargs).is_err());
        assert!(parse_single_id_arg(SYNC_ONE_REPO_TASK, &json!([""]), &kwargs).is_err());
        assert!(parse_single_id_arg(SYNC_ONE_REPO_TASK, &json!(["a"]), &json!({"x": 1})).is_err());
    }

    // Transport construction: empty tokens fail closed; `api_base` keeps
    // its trailing slash stripped like Python's `rstrip("/")`.
    #[test]
    fn transport_connect_matches_python_ctor() {
        assert_eq!(
            GithubRestClient::connect("", 30, GITHUB_API_BASE).unwrap_err(),
            GithubError::Auth("empty token".to_owned())
        );
        let client = GithubRestClient::connect("t", 30, "https://h.test///").expect("builds");
        assert_eq!(client.api_base, "https://h.test");
    }

    // Live transport against a hermetic stub: paginated listing follows
    // `rel="next"`, the POST carries the JSON body, and a 401 maps to
    // `Auth` with the response text.
    #[test]
    fn transport_round_trips_like_requests() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::Mutex;

        let listener = TcpListener::bind("127.0.0.1:0").expect("binds");
        let port = listener.local_addr().expect("addr").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_server = seen.clone();
        let server = std::thread::spawn(move || {
            for stream in listener.incoming().take(4) {
                let mut stream = stream.expect("accepts");
                let mut request = Vec::new();
                let mut byte = [0u8; 1];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    if stream.read(&mut byte).expect("reads") == 0 {
                        break;
                    }
                    request.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&request).into_owned();
                seen_server
                    .lock()
                    .expect("locks")
                    .push(head.lines().next().unwrap_or("").to_owned());
                let (status, body, link) =
                    if head.starts_with("GET ") && head.contains("/issues/comments") {
                        ("200 OK", r#"[]"#.to_owned(), None)
                    } else if head.starts_with("GET ") && head.contains("page=2") {
                        ("200 OK", r#"[{"number": 2}]"#.to_owned(), None)
                    } else if head.starts_with("GET ") {
                        (
                            "200 OK",
                            r#"[{"number": 1, "pull_request": {}}]"#.to_owned(),
                            Some(format!(
                                "<http://127.0.0.1:{port}/repos/o/r/issues?page=2>; rel=\"next\""
                            )),
                        )
                    } else if head.starts_with("POST ") {
                        ("201 Created", r#"{"id": 4242}"#.to_owned(), None)
                    } else {
                        ("401 Unauthorized", "Bad credentials".to_owned(), None)
                    };
                let mut response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                if let Some(link) = link {
                    response.push_str(&format!("Link: {link}\r\n"));
                }
                response.push_str(&format!("\r\n{body}"));
                stream.write_all(response.as_bytes()).expect("writes");
            }
        });

        let base = format!("http://127.0.0.1:{port}");
        let client = GithubRestClient::connect("t", 5, &base).expect("builds");
        let issues = client.list_all_open_issues("o", "r").expect("lists");
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0]["number"], json!(1));
        assert_eq!(issues[1]["number"], json!(2));
        let posted = client
            .post_issue_comment("o", "r", 1, "hello")
            .expect("posts");
        assert_eq!(posted["id"], json!(4242));
        let denied = client
            .list_all_repo_comments("o", "r")
            .expect("lists comments");
        assert!(denied.is_empty());
        server.join().expect("server drains");

        let lines = seen.lock().expect("locks");
        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("GET /repos/o/r/issues?"));
        assert!(lines[0].contains("state=open"));
        assert!(lines[1].starts_with("GET /repos/o/r/issues?page=2"));
        assert!(lines[2].starts_with("POST /repos/o/r/issues/1/comments"));
        assert!(lines[3].starts_with("GET /repos/o/r/issues/comments?"));
        assert!(lines[3].contains("direction=asc"));
    }

    // A 401 maps to `Auth` carrying the response text (the
    // `response.text` arm of `_request`).
    #[test]
    fn transport_maps_401_to_auth() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("binds");
        let port = listener.local_addr().expect("addr").port();
        let server = std::thread::spawn(move || {
            for stream in listener.incoming().take(1) {
                let mut stream = stream.expect("accepts");
                let mut request = Vec::new();
                let mut byte = [0u8; 1];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    if stream.read(&mut byte).expect("reads") == 0 {
                        break;
                    }
                    request.push(byte[0]);
                }
                let body = "Bad credentials";
                let response = format!(
                    "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).expect("writes");
            }
        });
        let client =
            GithubRestClient::connect("t", 5, &format!("http://127.0.0.1:{port}")).expect("builds");
        assert_eq!(
            client.list_all_open_issues("o", "r").unwrap_err(),
            GithubError::Auth("Bad credentials".to_owned())
        );
        server.join().expect("server drains");
    }

    // Every statement parses as Postgres, and the read statements are
    // SELECTs over the exact tables the ORM touches.
    #[test]
    fn all_statements_parse_as_postgres() {
        for sql in [
            ENABLED_SYNCS_SQL,
            SYNC_SCAN_SQL,
            PROJECT_WORKSPACE_SQL,
            ISSUE_LOOKUP_SQL,
            ISSUE_STATE_SQL,
            ADOPT_STATE_SQL,
            INSERT_ISSUE_SQL,
            UPDATE_ISSUE_SQL,
            INSERT_ISSUE_SYNC_SQL,
            UPDATE_ISSUE_SYNC_SQL,
            GITHUB_SYNC_LOOKUP_SQL,
            INSERT_ISSUE_SEQUENCE_SQL,
            MAX_SEQUENCE_SQL,
            MAX_SORT_ORDER_SQL,
            ADVISORY_LOCK_SQL,
            DEFAULT_POD_SQL,
            ISSUE_COMMENT_LOOKUP_SQL,
            INSERT_DESCRIPTION_SQL,
            INSERT_ISSUE_COMMENT_SQL,
            UPDATE_ISSUE_COMMENT_SQL,
            UPDATE_COMMENT_DESCRIPTION_SQL,
            COMMENT_SYNC_LOOKUP_SQL,
            INSERT_COMMENT_SYNC_SQL,
            UPDATE_COMMENT_SYNC_SQL,
            SYNC_SUCCESS_SQL,
            SYNC_ERROR_SQL,
            UPDATE_GITHUB_SYNC_METADATA_SQL,
            RECONCILE_LIST_SQL,
            COMPLETION_LOOKUP_SQL,
            "SELECT metadata FROM github_issue_syncs WHERE id = $1",
            default_state_sql(true),
            default_state_sql(false),
            save_state_sql(true),
            save_state_sql(false),
        ] {
            parse(sql);
        }
    }

    // Read predicates mirror the ORM scopes: soft-delete filters on every
    // base table, none on the joined tables; exact key columns.
    #[test]
    fn read_predicates_mirror_orm_scopes() {
        assert!(ENABLED_SYNCS_SQL.contains("github_repository_syncs"));
        assert!(ENABLED_SYNCS_SQL.contains("is_sync_enabled = TRUE"));
        assert!(ENABLED_SYNCS_SQL.contains("deleted_at IS NULL"));

        assert!(is_select(&parse(SYNC_SCAN_SQL)));
        assert!(SYNC_SCAN_SQL.contains("github_repository_syncs s"));
        assert!(SYNC_SCAN_SQL.contains("INNER JOIN github_repositories r"));
        assert!(SYNC_SCAN_SQL.contains("INNER JOIN workspace_integrations wi"));
        // No tombstone filter on the joined tables (Django
        // `select_related` semantics); only the base row is scoped.
        assert_eq!(SYNC_SCAN_SQL.matches("deleted_at IS NULL").count(), 1);

        assert!(ISSUE_LOOKUP_SQL.contains("external_source = 'github'"));
        assert!(ISSUE_LOOKUP_SQL.contains("project_id = $1"));

        assert!(is_select(&parse(COMPLETION_LOOKUP_SQL)));
        for table in [
            "github_issue_syncs s",
            "github_repository_syncs rs",
            "github_repositories r",
            "workspace_integrations wi",
            "issues i",
            "workspaces w",
        ] {
            assert!(COMPLETION_LOOKUP_SQL.contains(table), "{table}");
        }
        assert_eq!(
            COMPLETION_LOOKUP_SQL.matches("deleted_at IS NULL").count(),
            1
        );

        // The prior-state snapshot is deliberately UNSCOPED
        // (`Issue.all_objects`, a plain Manager): soft-deleted rows still
        // resolve. Asserted in `github_signals` tests; noted here so the
        // contrast with every scoped lookup above is explicit.
        assert!(RECONCILE_LIST_SQL.contains("repository_sync_id = $1"));
    }

    // Write shapes match the ORM calls: named-column updates only, full
    // inserts, audit NULLs on the sync rows, actor restamps on mirrors.
    #[test]
    fn write_shapes_match_orm_calls() {
        // Success stamps both columns; faults write only the error.
        assert!(SYNC_SUCCESS_SQL.contains("last_synced_at = $2"));
        assert!(SYNC_SUCCESS_SQL.contains("last_sync_error = ''"));
        assert!(!SYNC_ERROR_SQL.contains("last_synced_at"));
        assert!(SYNC_ERROR_SQL.contains("last_sync_error = $2"));
        // Neither touches `updated_at` (`save(update_fields=[…])`).
        assert!(!SYNC_SUCCESS_SQL.contains("updated_at"));
        assert!(!SYNC_ERROR_SQL.contains("updated_at"));

        // Sync rows: audit NULL on both arms (no restamp).
        assert!(INSERT_ISSUE_SYNC_SQL.contains("NULL, NULL, NULL"));
        assert!(UPDATE_ISSUE_SYNC_SQL.contains("created_by_id = NULL"));
        assert!(INSERT_COMMENT_SYNC_SQL.contains("NULL, NULL, NULL"));
        assert!(UPDATE_COMMENT_SYNC_SQL.contains("created_by_id = NULL"));

        // Mirror rows: actor restamp collapsed into the update.
        assert!(UPDATE_ISSUE_SQL.contains("created_by_id = $6"));
        assert!(UPDATE_ISSUE_COMMENT_SQL.contains("created_by_id = $8"));

        // Metadata-only writes touch nothing else.
        assert_eq!(
            UPDATE_GITHUB_SYNC_METADATA_SQL,
            "UPDATE github_issue_syncs SET metadata = $2 WHERE id = $1"
        );

        // Comment-sync key is (issue_sync, comment), not the remote id.
        assert!(COMMENT_SYNC_LOOKUP_SQL.contains("issue_sync_id = $1"));
        assert!(COMMENT_SYNC_LOOKUP_SQL.contains("comment_id = $2"));
    }

    // Fixture coverage: every behavior block the fixture issue recorded
    // maps to a function or statement above (trace per fixture block).
    #[test]
    fn fixture_before_after_is_fully_covered() {
        let task = fixture("tasks/github_sync_task.before_after.json");
        // `_markdown_to_html` / `_safe_render` live in `super::git_sync`
        // (imported, byte-identical); the golden vectors pin them there.
        assert!(task.get("_markdown_to_html").is_some());
        assert!(task.get("_safe_render").is_some());
        // `_upsert_issue`: name prefix, ignored default_state, PR-skip.
        assert_eq!(github_issue_name(7, "T"), "[github_7] T");
        assert!(task["_upsert_issue"]["default_state_param"]
            .as_str()
            .unwrap_or("")
            .contains("IGNORED"));
        // `_upsert_comment`: prefix shape + parent resolution.
        let (safe_html, _) = safe_render(Some("x"));
        assert!(mirror_comment_html(&safe_html).starts_with("<p>[Github] </p>"));
        assert!(task["_upsert_comment"]["parent_resolution"]
            .as_str()
            .unwrap_or("")
            .contains("parse_issue_number_from_url"));
        // Error paths: missing token, 4xx, unexpected+retry.
        assert!(task["error_paths"]["missing_token"]
            .as_str()
            .unwrap_or("")
            .contains(MISSING_TOKEN_ERROR));
        assert_eq!(MAX_RETRIES, 3);
        // Reconcile: same flag semantics as the git poller.
        assert!(
            task["reconcile_upstream_gone"]["same_flag_clear_semantics_as_git_sync"]
                .as_str()
                .is_some()
        );
    }

    // Registry: the three names route locally once registered; anything
    // else stays Python-owned (forwarded over AMQP). The pool is lazy
    // (never connects), so registration is asserted without a database —
    // but even a lazy pool needs a Tokio context to build.
    #[tokio::test]
    async fn registry_routes_owned_local_and_rest_to_python() {
        let mut registry = Registry::new();
        for task in TASK_NAMES {
            assert_eq!(route_for(&registry, task), Route::PythonOwned);
        }
        assert_eq!(
            route_for(&registry, "pi_dash.bgtasks.other.task"),
            Route::PythonOwned
        );
        let pool = PgPool::connect_lazy("postgres://localhost:1/unused")
            .expect("lazy pool builds without connecting");
        register_github_sync_tasks(
            &mut registry,
            pool,
            LiveTransports::with_keyring(pidash_db::config::encryption::Keyring::from_secret(
                "test",
            )),
        );
        for task in TASK_NAMES {
            assert_eq!(route_for(&registry, task), Route::Local);
        }
        assert_eq!(
            route_for(&registry, "pi_dash.bgtasks.other.task"),
            Route::PythonOwned
        );
    }
}
