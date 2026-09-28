#![forbid(unsafe_code)]

//! Provider-neutral git issue sync poller (D-05, jobs layer).
//!
//! Ports `apps/api/pi_dash/bgtasks/git_sync_task.py:30-323` (translation
//! only): the seven helpers (`_is_enabled`, `_project_default_state`,
//! `_remote_repository`, `_display_name`, `_upsert_issue`, `_upsert_comment`,
//! `_reconcile_upstream_gone`), the three Celery tasks
//! (`sync_all_bindings`, `sync_one_binding`, `post_completion_comment`),
//! and the owned beat entry `github-issue-sync-every-4h`
//! (`apps/api/pi_dash/celery.py:113-118`).
//!
//! Layering: the provider DTOs, registry facts, error hierarchy and adapter
//! contract live in `pidash-types` (PIDASHCONV-140); the concrete adapters
//! in `pidash-services` (PIDASHCONV-141/143); the table shapes in
//! `pidash-db` (stage 5 models). This module owns the task bodies, the
//! beat selection, the Celery wire payloads, the retry/ETA policy, and —
//! per the adapters' documented seam ("a real HTTP transport lands with the
//! tasks layer") — the two live transports ([`ReqwestGithubClient`],
//! [`ReqwestGitLabTransport`]).
//!
//! SQL semantics mirror the Django ORM arm for arm (see each builder):
//! soft-delete scoping follows the managers (`SoftDeletionManager`,
//! `StateManager` which additionally excludes `group = 'triage'`,
//! `PodManager`); `select_related` joins are plain `INNER JOIN`s with no
//! tombstone filter on the joined tables, exactly like Django; `QuerySet`
//! updates write only the named columns (no `auto_now` touch); creates
//! replicate the `save()` side effects (`Issue.save`: pod/state/sequence/
//! sort-order/description-stripped resolution; `IssueComment.save`:
//! description row + change-tracked description update).
//!
//! Redelivery safety: every write is keyed on a natural unique (`binding,
//! external_iid`, `issue + provider + external_id`, `issue_sync +
//! external_id`) via select-then-insert-or-update — the same race window as
//! Python's `update_or_create` (a concurrent duplicate errors and retries,
//! never double-applies). Queue-level dedup comes from the F-09 plane: a
//! failure requeues with [`Verdict::Retry`], a spent budget parks as
//! `failed`; nothing is ever dropped ([`crate::worker`]).
//!
//! PORT BUGS (ported, not fixed):
//!
//! * `_ = default_state`: `_upsert_issue` ignores its `default_state`
//!   argument (`git_sync_task.py:115`); the new row's state comes from
//!   `Issue.save()` re-deriving it (non-triage default). [`resolve_state`]
//!   keeps the dead parameter shape so the layer below stays swappable.
//! * The save-time state query filters `is_triage = false` while
//!   `_project_default_state` excludes `group = 'triage'` instead — two
//!   different notions of "triage" that agree on seeded data.
//! * Create recomputes `description_stripped` via `strip_tags`; the update
//!   path keeps the `_safe_render` value (no `save()`, no recompute).
//! * The `.strip()` on the mirror `comment_stripped`
//!   (`git_sync_task.py:153`) is dead: `update_or_create` always runs the
//!   full `IssueComment.save()`, which recomputes the column from
//!   `comment_html` without stripping (`issue.py:606`).
//! * The new-comment `Description` row keeps entities (`Description.save`
//!   re-strips with Django semantics, `description.py:22-28`) while the
//!   description refresh on the update path writes the decoded recompute
//!   (`filter().update`, no `save()`).
//! * The metadata read-modify-writes (`_reconcile_upstream_gone`, the
//!   completion guard) are non-atomic, as in Python.
//! * `GIT_SYNC_ENABLED` exists nowhere in `settings/common.py`, so the
//!   `getattr` chain always falls through to `GITHUB_SYNC_ENABLED` in
//!   practice; [`git_sync_enabled`] keeps both lookups in order.
//!
//! Fixture ids replayed by the unit tests below:
//! `rust-api/fixtures/integrations/tasks/git_sync_task.before_after.json`
//! (the issue names it `git_sync.json`; the recorded behavior lives under
//! the `git_sync_task.before_after.json` name) and
//! `rust-api/fixtures/integrations/beat.json`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde_json::{json, Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_db::config::encryption::Keyring;
use pidash_services::integrations::accounts::account_credential;
use pidash_services::integrations::adapters_github::{
    ClientAuth, GitHubAdapter, GithubClient, GithubError,
};
use pidash_services::integrations::adapters_gitlab::{
    GitLabAdapter, GitLabRequest, GitLabResponse, GitLabTransport,
};
use pidash_types::integrations::registry::{provider_payload, resolve_adapter_key};
use pidash_types::integrations::{
    GitProviderAdapter, GitProviderError, RemoteComment, RemoteIssue, RemoteRepository,
};

use super::super::tasks_mail::mail_send::{django_escape, strip_tags};
use crate::celery::{format_eta, CeleryTaskMessage};
use crate::queue::{enqueue, JobRow, NewJob};
use crate::schedule::{beat_schedule, BeatEntry};
use crate::worker::{HandlerError, Registry, Verdict};

// ---------------------------------------------------------------------------
// Task + beat identity
// ---------------------------------------------------------------------------

/// `sync_all_bindings` (`git_sync_task.py:213`, `@shared_task` with no name
/// override: the Celery name is the module path).
pub const SYNC_ALL_BINDINGS_TASK: &str = "pi_dash.bgtasks.git_sync_task.sync_all_bindings";
/// `sync_one_binding` (`git_sync_task.py:223`, `bind=True, max_retries=3`).
pub const SYNC_ONE_BINDING_TASK: &str = "pi_dash.bgtasks.git_sync_task.sync_one_binding";
/// `post_completion_comment` (`git_sync_task.py:281`, plain `@shared_task`:
/// never retries — every fault is recorded and returned).
pub const POST_COMPLETION_COMMENT_TASK: &str =
    "pi_dash.bgtasks.git_sync_task.post_completion_comment";

/// Every Celery task name this module owns, in Python definition order.
pub const TASK_NAMES: [&str; 3] = [
    SYNC_ALL_BINDINGS_TASK,
    SYNC_ONE_BINDING_TASK,
    POST_COMPLETION_COMMENT_TASK,
];

/// `bind=True, max_retries=3` (`git_sync_task.py:223`). The queue row
/// carries the same budget: [`NewJob::new`] defaults to
/// [`DEFAULT_MAX_RETRIES`], which is 3 for exactly this task.
pub const MAX_RETRIES: i32 = 3;

/// Beat entry name (`celery.py:116`). The legacy `github-` prefix is kept
/// so django-celery-beat updates the existing rows instead of running the
/// old GitHub poller and this poller in parallel.
pub const GIT_SYNC_BEAT: &str = "github-issue-sync-every-4h";

/// The owned beat entry, selected from the F-09 schedule
/// ([`crate::schedule::beat_schedule`], already transcribed from
/// `celery.py`): `github-issue-sync-every-4h` firing
/// `sync_all_bindings` on `crontab(minute=0, hour=*/4)`. Every other entry
/// belongs to its own domain — selection, not a fork.
pub fn beat_entries() -> Vec<BeatEntry> {
    beat_schedule()
        .into_iter()
        .filter(|entry| entry.name == GIT_SYNC_BEAT)
        .collect()
}

// ---------------------------------------------------------------------------
// Kill switch + display names
// ---------------------------------------------------------------------------

/// Read one `get_config`-style boolean: missing means `default`, otherwise
/// the value lowercased must equal `"true"` (`settings/common.py:441`
/// builds `GITHUB_SYNC_ENABLED` as
/// `get_config("GITHUB_SYNC_ENABLED", "true").lower() == "true"`).
fn env_flag(name: &str, default: bool) -> Option<bool> {
    std::env::var(name)
        .ok()
        .map(|value| value.to_lowercase() == "true")
        .or(Some(default))
}

/// Instance-level kill switch (`git_sync_task.py:30-32`):
/// `getattr(settings, "GIT_SYNC_ENABLED", getattr(settings,
/// "GITHUB_SYNC_ENABLED", True))`. `GIT_SYNC_ENABLED` is read first when
/// present; otherwise `GITHUB_SYNC_ENABLED`; both default to enabled.
pub fn git_sync_enabled() -> bool {
    match std::env::var("GIT_SYNC_ENABLED").ok() {
        Some(value) => value.to_lowercase() == "true",
        None => env_flag("GITHUB_SYNC_ENABLED", true).unwrap_or(true),
    }
}

/// Python `str.title()`: the first cased character of each word uppercases
/// (titlecase), the rest lowercase; a word starts after any non-cased
/// character (`'my-provider'.title() == 'My-Provider'`,
/// `'foo2bar'.title() == 'Foo2Bar'`, verified against CPython).
pub fn py_title(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut previous_is_cased = false;
    for ch in value.chars() {
        if previous_is_cased {
            out.extend(ch.to_lowercase());
        } else if ch.is_uppercase() || ch.is_lowercase() {
            // `str.title()` prefers titlecase for the few characters that
            // have one (e.g. U+01C8 → U+01C8, not U+01C4); uppercase is the
            // std approximation. Provider names are ASCII, where the two
            // agree exactly.
            out.extend(ch.to_uppercase());
        } else {
            out.push(ch);
        }
        previous_is_cased = ch.is_uppercase() || ch.is_lowercase();
    }
    out
}

/// Provider display name (`git_sync_task.py:59-63`): the registered
/// adapter's `display_name` (case-insensitive lookup), falling back to
/// `provider.title()` on `KeyError`.
pub fn display_name(provider: &str) -> String {
    match resolve_adapter_key(provider) {
        Ok(key) => provider_payload()
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.display_name.to_owned())
            .unwrap_or_else(|| py_title(provider)),
        Err(_) => py_title(provider),
    }
}

// ---------------------------------------------------------------------------
// Body rendering (`github_sync_task._safe_render`, reused by both upserts)
// ---------------------------------------------------------------------------

/// Render an upstream markdown body to minimal HTML
/// (`github_sync_task.py:68-80`): falsy bodies become `"<p></p>"`,
/// paragraphs split on blank lines, each escaped (defense in depth — the
/// sanitizer below is the second layer) with intra-paragraph newlines as
/// `<br/>`. Paragraph-per-blank-line is the whole MVP markdown engine.
pub fn markdown_to_html(body: Option<&str>) -> String {
    match body.filter(|text| !text.is_empty()) {
        None => "<p></p>".to_owned(),
        Some(text) => {
            let paragraphs: Vec<&str> = text
                .split("\n\n")
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect();
            if paragraphs.is_empty() {
                return "<p></p>".to_owned();
            }
            paragraphs
                .iter()
                .map(|part| format!("<p>{}</p>", django_escape(part).replace('\n', "<br/>")))
                .collect::<String>()
        }
    }
}

/// Extra tags beyond the sanitizer defaults
/// (`content_validator.py:72-78`): editor node/tag names.
fn sanitizer_tags() -> HashSet<&'static str> {
    let mut tags: HashSet<&'static str> = ammonia::Builder::default()
        .clone_tags()
        .into_iter()
        .collect();
    tags.insert("mention-component");
    tags.insert("label");
    tags.insert("input");
    tags.insert("image-component");
    tags
}

/// Generic (`*`) attributes (`content_validator.py:83-113`).
fn sanitizer_generic_attributes() -> HashSet<&'static str> {
    [
        "class",
        "id",
        "title",
        "role",
        "aria-label",
        "aria-hidden",
        "style",
        "start",
        "type",
        "xmlns",
        "data-tight",
        "data-node-type",
        "data-type",
        "data-checked",
        "data-background-color",
        "data-text-color",
        "data-name",
        "data-id",
        "data-icon-name",
        "data-icon-color",
        "data-background",
        "data-emoji-unicode",
        "data-emoji-url",
        "data-logo-in-use",
        "data-block-type",
    ]
    .into_iter()
    .collect()
}

/// Per-tag attributes (`content_validator.py:114-157`). The `*` entry is
/// kept out of this map: it is installed as the generic set instead (nh3
/// reads the `"*"` key as generic attributes).
fn sanitizer_tag_attributes() -> HashMap<&'static str, HashSet<&'static str>> {
    let mut map: HashMap<&'static str, HashSet<&'static str>> = HashMap::new();
    map.insert("a", ["href", "target"].into_iter().collect());
    map.insert(
        "image-component",
        [
            "id",
            "width",
            "height",
            "aspectRatio",
            "aspectratio",
            "src",
            "alignment",
            "status",
        ]
        .into_iter()
        .collect(),
    );
    map.insert(
        "img",
        [
            "width",
            "height",
            "aspectRatio",
            "aspectratio",
            "alignment",
            "src",
            "alt",
            "title",
        ]
        .into_iter()
        .collect(),
    );
    map.insert(
        "mention-component",
        ["id", "entity_identifier", "entity_name"]
            .into_iter()
            .collect(),
    );
    map.insert(
        "th",
        ["colspan", "rowspan", "colwidth", "background", "style"]
            .into_iter()
            .collect(),
    );
    map.insert(
        "td",
        [
            "colspan",
            "rowspan",
            "colwidth",
            "background",
            "textColor",
            "textcolor",
            "style",
        ]
        .into_iter()
        .collect(),
    );
    map.insert(
        "tr",
        ["background", "textColor", "textcolor", "style"]
            .into_iter()
            .collect(),
    );
    map.insert("pre", ["language"].into_iter().collect());
    map.insert("code", ["language", "spellcheck"].into_iter().collect());
    map.insert("input", ["type", "checked"].into_iter().collect());
    map
}

/// Allowed URL schemes (`content_validator.py:159`).
fn sanitizer_url_schemes() -> HashSet<&'static str> {
    ["http", "https", "mailto", "tel"].into_iter().collect()
}

/// Sanitize rendered HTML (`validate_html_content`,
/// `content_validator.py:211-243`): `nh3.clean` with the transcribed
/// allowlist. nh3 is Python bindings for ammonia, so the same engine with
/// the same configuration renders byte-identical output (pinned by the
/// golden vectors in `tests`, generated from `nh3==0.2.18` per
/// `apps/api/requirements/base.txt:96`). `None` on sanitizer failure, like
/// the `(False, msg, None)` triple — the caller falls back to escaped text.
fn sanitize_html(rendered: &str) -> Option<String> {
    let mut builder = ammonia::Builder::default();
    builder
        .tags(sanitizer_tags())
        .generic_attributes(sanitizer_generic_attributes())
        .tag_attributes(sanitizer_tag_attributes())
        .url_schemes(sanitizer_url_schemes());
    Some(builder.clean(rendered).to_string())
}

/// `pi_dash.utils.html_processor.strip_tags` (`html_processor.py:11-31`):
/// the `MLStripper` (`HTMLParser` with `convert_charrefs=True`) — tags
/// dropped, character references in text decoded. This is NOT Django's
/// `strip_tags` (regex, entities kept), which
/// [`tasks_mail::mail_send::strip_tags`][crate::tasks_mail] already ports:
/// every call site this module mirrors (`_safe_render`,
/// `github_sync_task.py:40`; `Issue.save` / `IssueComment.save`,
/// `issue.py:21`) imports the html_processor one. Tag removal reuses the
/// shared scanner; only the charref layer is new.
pub fn strip_html_text(html: &str) -> String {
    decode_char_refs(&strip_tags(html))
}

/// Decode one `&...;` reference body (no leading `&`, no trailing `;`)
/// the way `convert_charrefs` does: the named references a serializer can
/// emit (`&amp; &lt; &gt; &quot; &apos; &nbsp;` — Django's `escape` and the
/// html5ever serializer never emit any other named reference literally)
/// plus decimal/hex numeric references (unrepresentable code points become
/// U+FFFD). Anything else stays verbatim. `None` means "not a reference".
fn decode_char_ref(body: &str) -> Option<char> {
    if let Some(stripped) = body.strip_prefix('#') {
        let (digits, radix) = match stripped.strip_prefix(['x', 'X']) {
            Some(hex) => (hex, 16),
            None => (stripped, 10),
        };
        if digits.is_empty()
            || !digits
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && (radix == 16 || b.is_ascii_digit()))
        {
            return None;
        }
        // The HTML5 numeric-reference table maps NUL to U+FFFD (like
        // surrogates and out-of-range code points); every other value
        // yields its character. (CPython additionally drops C0 controls
        // and noncharacters, but a serializer never emits those as
        // references, so they stay out of this function's domain.)
        return Some(match u32::from_str_radix(digits, radix).ok() {
            Some(0) | None => '\u{FFFD}',
            Some(code) => char::from_u32(code).unwrap_or('\u{FFFD}'),
        });
    }
    match body {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => None,
    }
}

/// `&...;` decoding over tag-free text (`convert_charrefs=True`): known
/// references decode, unknown or unterminated sequences stay verbatim.
/// Single left-to-right pass with no rescan of replacements, exactly like
/// the parser (`&amp;amp;` becomes `&amp;`, never `&`). Only
/// semicolon-terminated references decode: every input here is serializer
/// output (ammonia / Django `escape`), which always terminates references
/// with `;`, so the legacy no-semicolon forms are unreachable.
pub fn decode_char_refs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after
            .find(';')
            .map(|end| (&after[..end], &after[end + 1..]))
        {
            Some((body, tail)) if !body.is_empty() => match decode_char_ref(body) {
                Some(ch) => {
                    out.push(ch);
                    rest = tail;
                }
                None => {
                    out.push('&');
                    rest = after;
                }
            },
            _ => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Render an upstream body to `(html, stripped)`
/// (`github_sync_task.py:83-95`): markdown render, sanitized through the
/// same allow-list that protects user-written content, falling back to
/// fully-escaped plain text if the sanitizer rejects outright; the
/// stripped form is html_processor [`strip_html_text`] of the safe HTML
/// (`github_sync_task.py:40,95`).
pub fn safe_render(body: Option<&str>) -> (String, String) {
    let rendered = markdown_to_html(body);
    let safe_html = match sanitize_html(&rendered) {
        Some(clean) => clean,
        None => format!(
            "<p>{}</p>",
            django_escape(body.unwrap_or("")).replace('\n', "<br/>")
        ),
    };
    let stripped = strip_html_text(&safe_html);
    (safe_html, stripped)
}

// ---------------------------------------------------------------------------
// Small pure helpers
// ---------------------------------------------------------------------------

/// Truncate to `n` characters (`name[:255]`, `str(e)[:900]` …): Python
/// slices by code point, so a multibyte tail is cut, never split.
pub fn truncate_chars(value: &str, n: usize) -> String {
    value.chars().take(n).collect()
}

/// Mirror title: `f"[{provider}_{iid}] {title}"[:255]`
/// (`git_sync_task.py:74`).
pub fn prefixed_issue_name(provider: &str, issue_iid: &str, title: &str) -> String {
    truncate_chars(&format!("[{provider}_{issue_iid}] {title}"), 255)
}

/// Retry delay for `sync_one_binding` (`git_sync_task.py:278`):
/// `countdown=60 * (2 ** self.request.retries)`. `retries` is the worker's
/// used-attempts count ([`JobRow::attempts`]); saturating so a corrupt row
/// parks instead of panicking.
pub fn retry_countdown_secs(retries: u32) -> u64 {
    60u64.saturating_mul(2u64.saturating_pow(retries.min(16)))
}

/// The Python exception class name for a provider fault, as interpolated
/// into `last_sync_error` / `completion_comment_error`
/// (`git_sync_task.py:269,312`).
pub fn provider_error_label(error: &GitProviderError) -> &'static str {
    match error {
        GitProviderError::General(_) => "GitProviderError",
        GitProviderError::Auth(_) => "GitProviderAuthError",
        GitProviderError::Permission(_) => "GitProviderPermissionError",
        GitProviderError::NotFound(_) => "GitProviderNotFoundError",
    }
}

/// `f"{TypeName}: {msg}"[:900]` for binding error recording
/// (`git_sync_task.py:269`).
pub fn binding_error_text(error: &GitProviderError) -> String {
    truncate_chars(
        &format!("{}: {}", provider_error_label(error), error.message()),
        900,
    )
}

/// `str(e)[:1000]` for unexpected faults (`git_sync_task.py:276`).
pub fn unexpected_error_text(message: &str) -> String {
    truncate_chars(message, 1000)
}

/// `f"{TypeName}: {msg}"[:500]` / `str(e)[:500]` for the completion path
/// (`git_sync_task.py:312,317`).
pub fn completion_error_text(label: &str, message: &str) -> String {
    truncate_chars(&format!("{label}: {message}"), 500)
}

/// Absolute deep-link to an issue in the Pi Dash UI
/// (`github_sync_task.py:98-105`): `{base}/{workspace_slug}/projects/
/// {project_id}/issues/{issue_id}` with a trailing slash stripped off the
/// base. `base` is `WEB_URL` or `APP_BASE_URL` (resolution is the caller's;
/// `post_completion_comment` reads the same two settings in order).
pub fn pidash_issue_url(
    base: &str,
    workspace_slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> String {
    format!(
        "{}/{workspace_slug}/projects/{project_id}/issues/{issue_id}",
        base.trim_end_matches('/'),
    )
}

/// `WEB_URL or APP_BASE_URL` (`github_sync_task.py:101`): first present
/// non-empty setting wins.
pub fn completion_base_url(web_url: Option<&str>, app_base_url: Option<&str>) -> Option<String> {
    web_url
        .filter(|value| !value.is_empty())
        .or_else(|| app_base_url.filter(|value| !value.is_empty()))
        .map(str::to_owned)
}

/// The completion comment body (`git_sync_task.py:300`).
pub fn completion_body(issue_url: &str) -> String {
    format!("This issue has been completed in Pi Dash: {issue_url}")
}

/// `convert_uuid_to_integer` (`db/../utils/uuid.py:19-26`): the
/// transaction advisory-lock key for per-project issue creation —
/// `sha256(str(uuid))`, first 8 bytes big-endian signed. `Uuid::to_string`
/// renders the same lowercase hyphenated form Python's `str()` does.
pub fn advisory_lock_key(project_id: &Uuid) -> i64 {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(project_id.to_string().as_bytes());
    i64::from_be_bytes(digest[..8].try_into().expect("sha256 is 32 bytes"))
}

// ---------------------------------------------------------------------------
// SQL (mirrors the ORM call-for-call; `$n` binds filled by the handlers)
// ---------------------------------------------------------------------------

/// Enabled-binding id set (`git_sync_task.py:218`):
/// `GitRepositoryBinding.objects.filter(is_sync_enabled=True)` — the
/// default manager scopes `deleted_at IS NULL`; no ordering (Django
/// iterates the filter in database order).
pub const ENABLED_BINDINGS_SQL: &str =
    "SELECT id FROM git_repository_bindings WHERE is_sync_enabled = TRUE AND deleted_at IS NULL";

/// One binding with its `select_related` closure
/// (`git_sync_task.py:229-235`): repository, provider_account, project,
/// workspace, actor. `select_related` joins are plain `INNER JOIN`s —
///
/// Django does NOT apply the related managers' tombstone filters here —
/// so neither does this statement: only the base binding row is
/// soft-delete scoped. `$1` is the binding id; a miss is the
/// `DoesNotExist` no-op.
pub const BINDING_SCAN_SQL: &str = "SELECT b.id AS binding_id, b.project_id, b.workspace_id, b.actor_id, \
    r.provider, r.external_id AS repo_external_id, r.namespace AS repo_namespace, r.name AS repo_name, \
    r.full_name AS repo_full_name, r.web_url AS repo_web_url, \
    r.clone_url_http AS repo_clone_http, r.clone_url_ssh AS repo_clone_ssh, \
    r.default_branch AS repo_default_branch, r.is_private AS repo_is_private, r.metadata AS repo_metadata, \
    a.id AS account_id, a.auth_type AS account_auth_type, a.host_url AS account_host_url, \
    a.credential_config AS account_credential_config \
    FROM git_repository_bindings b \
    INNER JOIN git_repositories r ON r.id = b.repository_id \
    INNER JOIN git_provider_accounts a ON a.id = b.provider_account_id \
    WHERE b.id = $1 AND b.deleted_at IS NULL";

/// `_project_default_state` (`git_sync_task.py:35-39`): default state
/// first, else first state — both through `StateManager` (live rows only,
/// `group <> 'triage'`), Meta ordering `sequence ASC`. `$1` is the project
/// id. `default_state_sql(true)` is tried first; on a miss,
/// `default_state_sql(false)`.
pub fn default_state_sql(prefer_default: bool) -> &'static str {
    if prefer_default {
        "SELECT id FROM states WHERE project_id = $1 AND deleted_at IS NULL AND \"group\" <> 'triage' AND \"default\" = TRUE ORDER BY sequence ASC LIMIT 1"
    } else {
        "SELECT id FROM states WHERE project_id = $1 AND deleted_at IS NULL AND \"group\" <> 'triage' ORDER BY sequence ASC LIMIT 1"
    }
}

/// Save-time state resolution (`Issue.save`, `issue.py:288-301`): unlike
/// `_project_default_state` this filters the `is_triage` boolean (not the
/// group) — `WHERE … AND is_triage = FALSE` (`~Q(is_triage=True)` on a
/// non-nullable column). `$1` is the project id; ordering `sequence ASC`.
pub fn save_state_sql(prefer_default: bool) -> &'static str {
    if prefer_default {
        "SELECT id, \"group\" FROM states WHERE project_id = $1 AND deleted_at IS NULL AND \"group\" <> 'triage' AND is_triage = FALSE AND \"default\" = TRUE ORDER BY sequence ASC LIMIT 1"
    } else {
        "SELECT id, \"group\" FROM states WHERE project_id = $1 AND deleted_at IS NULL AND \"group\" <> 'triage' AND is_triage = FALSE ORDER BY sequence ASC LIMIT 1"
    }
}

/// Default-pod resolution (`Issue.save`, `issue.py:277-286`):
/// `Pod.objects.filter(project_id, is_default=True).first()` — live rows
/// only (`PodManager`), Meta ordering `(-is_default, created_at)` which is
/// `created_at ASC` once every candidate is default. `$1` is the project
/// id. A miss leaves `assigned_pod_id` NULL.
pub const DEFAULT_POD_SQL: &str = "SELECT id FROM pod WHERE project_id = $1 AND is_default = TRUE AND deleted_at IS NULL ORDER BY created_at ASC LIMIT 1";

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

/// Mirror lookup (`git_sync_task.py:86-90`):
/// `GitIssueSync.objects.select_related("issue").filter(binding,
/// external_iid).first()`. `$1` the binding id, `$2` the iid.
pub const ISSUE_SYNC_LOOKUP_SQL: &str = "SELECT id, issue_id, metadata FROM git_issue_syncs WHERE binding_id = $1 AND external_iid = $2 AND deleted_at IS NULL";

/// Mirror-comment lookup (`git_sync_task.py:155-169`):
/// `IssueComment.objects.update_or_create(issue, external_source,
/// external_id, …)` — the existing row's render columns feed the
/// change-tracker comparison for the description side-table. `$1` the
/// issue id, `$2` the provider, `$3` the remote comment id.
pub const ISSUE_COMMENT_LOOKUP_SQL: &str = "SELECT id, comment_html, comment_stripped, comment_json, description_id FROM issue_comments WHERE issue_id = $1 AND external_source = $2 AND external_id = $3 AND deleted_at IS NULL";

/// Comment-sync lookup (`git_sync_task.py:177-179`):
/// `GitCommentSync.objects.update_or_create(issue_sync, external_id, …)`.
/// `$1` the issue-sync id, `$2` the remote comment id.
pub const COMMENT_SYNC_LOOKUP_SQL: &str = "SELECT id FROM git_comment_syncs WHERE issue_sync_id = $1 AND external_id = $2 AND deleted_at IS NULL";

/// Completion closure (`git_sync_task.py:287-293`): the sync row with its
/// `select_related` (`binding__repository`, `binding__provider_account`,
/// `issue`, `issue__workspace`) — plain inner joins, base row scoped.
/// `$1` is the issue-sync id; a miss is the `DoesNotExist` no-op.
pub const COMPLETION_LOOKUP_SQL: &str = "SELECT s.id AS sync_id, s.external_iid AS sync_external_iid, s.metadata AS sync_metadata, \
    b.id AS binding_id, \
    r.provider AS repo_provider, r.external_id AS repo_external_id, r.namespace AS repo_namespace, \
    r.name AS repo_name, r.full_name AS repo_full_name, r.web_url AS repo_web_url, \
    r.clone_url_http AS repo_clone_http, r.clone_url_ssh AS repo_clone_ssh, \
    r.default_branch AS repo_default_branch, r.is_private AS repo_is_private, r.metadata AS repo_metadata, \
    a.auth_type AS account_auth_type, a.host_url AS account_host_url, \
    a.credential_config AS account_credential_config, \
    i.id AS issue_id, i.project_id AS issue_project_id, \
    w.slug AS workspace_slug \
    FROM git_issue_syncs s \
    INNER JOIN git_repository_bindings b ON b.id = s.binding_id \
    INNER JOIN git_repositories r ON r.id = b.repository_id \
    INNER JOIN git_provider_accounts a ON a.id = b.provider_account_id \
    INNER JOIN issues i ON i.id = s.issue_id \
    INNER JOIN workspaces w ON w.id = i.workspace_id \
    WHERE s.id = $1 AND s.deleted_at IS NULL";

/// Reconcile candidate rows (`git_sync_task.py:201`): id, iid and metadata
/// of every live mirror on the binding. `$1` is the binding id.
pub const RECONCILE_LIST_SQL: &str =
    "SELECT id, external_iid, metadata FROM git_issue_syncs WHERE binding_id = $1 AND deleted_at IS NULL";

// ---------------------------------------------------------------------------
// Celery wire payloads
// ---------------------------------------------------------------------------

/// Parse the single positional id argument (`sync_one_binding(binding_id)`,
/// `post_completion_comment(issue_sync_id)`): `.delay(str(id))` arrives as
/// `args=[str]`, `kwargs={}`. Anything else is a malformed delivery —
/// Celery rejects invalid signatures without requeue, so the caller acks
/// (and warns) instead of retrying poison.
pub fn parse_single_id_arg(task: &str, args: &Value, kwargs: &Value) -> Result<String, String> {
    let id = args
        .as_array()
        .filter(|items| items.len() == 1)
        .and_then(|items| items[0].as_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{task}: expected args=[<id>], got {args}"))?;
    let kwargs_empty = kwargs
        .as_object()
        .map(|map| map.is_empty())
        .unwrap_or(false);
    if !kwargs_empty {
        return Err(format!("{task}: expected empty kwargs, got {kwargs}"));
    }
    Ok(id.to_owned())
}

/// One fan-out job per enabled binding (`git_sync_task.py:220`):
/// `sync_one_binding.delay(str(binding_id))` is `args=[str]`, `kwargs={}`
/// on the wire. `max_retries` rides the row default
/// ([`DEFAULT_MAX_RETRIES`] == [`MAX_RETRIES`], asserted in tests).
pub fn fanout_job(binding_id: &Uuid) -> NewJob {
    NewJob::new(
        SYNC_ONE_BINDING_TASK,
        json!([binding_id.to_string()]),
        json!({}),
    )
}

/// The Celery v2 message a fan-out row becomes on the wire: same task
/// name, `args=[str(id)]`, empty kwargs — byte-shape identical to the
/// Python `.delay(str(id))` call. Repeats [`crate::worker::dispatch`]'s
/// forward-path mapping arm for arm (that function is the runtime source
/// of truth; this one exists so tests can assert the wire contract without
/// a broker, following the `tasks_ticker::scan::fire_message` precedent).
pub fn fanout_message(binding_id: &Uuid) -> CeleryTaskMessage {
    let job = fanout_job(binding_id);
    let args = match job.args {
        Value::Array(items) => items,
        other => vec![other],
    };
    let kwargs = match job.kwargs {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    CeleryTaskMessage::new(SYNC_ONE_BINDING_TASK, args, kwargs)
}

// ---------------------------------------------------------------------------
// Live transports (the tasks-layer seam the adapters document)
// ---------------------------------------------------------------------------

/// `GITHUB_API_BASE` (`github_client.py:22`).
pub const GITHUB_API_BASE: &str = "https://api.github.com";
/// `DEFAULT_TIMEOUT_SECONDS` (`github_client.py:23`, `gitlab.py:34`).
pub const PROVIDER_TIMEOUT_SECS: u64 = 30;

/// Live GitHub REST transport (`utils/github_client.py:38-220`):
/// `Bearer` token auth, the `vnd.github+json` accept headers, exact status
/// mapping (401/403/404 → provider errors; anything else unsuccessful
/// surfaces as [`GithubError::Transport`], like `raise_for_status`'s
/// `HTTPError` passing through `_map_error` unchanged), and `Link
/// rel="next"` pagination.
#[derive(Debug, Clone)]
pub struct ReqwestGithubClient {
    http: reqwest::blocking::Client,
    token: String,
    api_base: String,
    timeout_secs: u64,
}

impl ReqwestGithubClient {
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
        // `_request` (`github_client.py:60-73`): exact mapping first,
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

    fn get_json(&self, url: &str) -> Result<Value, GithubError> {
        let response = self.checked(reqwest::Method::GET, url, None)?;
        response_json(response)
    }

    /// `_paginate` (`github_client.py:83-90`): follow `rel="next"` until
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
/// ASCII parameter values used here: `owner/collaborator/...` commas,
/// `open`, digits).
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

/// `_next_url` (`github_client.py:75-81`): parse the `next` URL from a
/// `Link` header — `re.match(r'\s*<([^>]+)>;\s*rel="next"', part)` per
/// comma-separated part.
pub fn next_link(header: &str) -> Option<String> {
    if header.is_empty() {
        return None;
    }
    for part in header.split(',') {
        let trimmed = part.trim_start();
        if !trimmed.starts_with('<') {
            continue;
        }
        let end = trimmed.find('>')?;
        let url = &trimmed[1..end];
        let rest = trimmed[end + 1..].trim_start();
        if !rest.starts_with(';') {
            continue;
        }
        let rel = rest[1..].trim_start();
        if rel.starts_with("rel=\"next\"") {
            return Some(url.to_owned());
        }
    }
    None
}

impl GithubClient for ReqwestGithubClient {
    /// `GithubClient(token=…)` / `GithubClient.for_installation(…)`
    /// (`github_client.py:39-52`): empty tokens fail closed with
    /// `Auth("empty token")`; installations mint via [`installation_token`].
    fn connect(auth: &ClientAuth) -> Result<Self, GithubError> {
        let http = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| GithubError::Transport(error.to_string()))?;
        match auth {
            ClientAuth::Token(token) => {
                if token.is_empty() {
                    return Err(GithubError::Auth("empty token".to_owned()));
                }
                Ok(Self {
                    http,
                    token: token.clone(),
                    api_base: GITHUB_API_BASE.to_owned(),
                    timeout_secs: PROVIDER_TIMEOUT_SECS,
                })
            }
            ClientAuth::Installation(installation_id) => {
                let token = installation_token(*installation_id, &http)?;
                Ok(Self {
                    http,
                    token,
                    api_base: GITHUB_API_BASE.to_owned(),
                    timeout_secs: PROVIDER_TIMEOUT_SECS,
                })
            }
        }
    }

    fn get_authenticated_user(&self) -> Result<Value, GithubError> {
        self.get_json(&format!("{}/user", self.api_base))
    }

    /// One page of `/user/repos` (`github_client.py:103-114`): the
    /// affiliation filter, `per_page` default 100, `sort=updated`;
    /// `has_next` is the `Link next` presence.
    fn list_user_repos(&self, page: i64) -> Result<(Vec<Value>, bool), GithubError> {
        let url = format!(
            "{}/user/repos?affiliation={}&per_page=100&sort=updated&page={page}",
            self.api_base,
            percent_encode("owner,collaborator,organization_member")
        );
        let response = self.checked(reqwest::Method::GET, &url, None)?;
        let link = response
            .headers()
            .get("link")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let repos = response_json_list(response)?;
        Ok((repos, next_link(&link).is_some()))
    }

    fn get_repo(&self, owner: &str, name: &str) -> Result<Value, GithubError> {
        self.get_json(&format!("{}/repos/{owner}/{name}", self.api_base))
    }

    /// Paginated `/issues?state=open` (`github_client.py:133-140`): PRs
    /// arrive alongside issues; the adapter filters them.
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

    fn list_issue_comments(
        &self,
        owner: &str,
        name: &str,
        issue_number: i64,
    ) -> Result<Vec<Value>, GithubError> {
        self.paginate(
            &format!("/repos/{owner}/{name}/issues/{issue_number}/comments"),
            &[
                ("per_page", "100".to_owned()),
                ("sort", "created".to_owned()),
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

    fn get_pull_request(&self, owner: &str, name: &str, number: i64) -> Result<Value, GithubError> {
        self.get_json(&format!(
            "{}/repos/{owner}/{name}/pulls/{number}",
            self.api_base
        ))
    }
}

/// GitHub App configuration (`github_app_auth.py:28-51`): `app_id` and
/// `app_slug` resolve from the environment with the same names as the
/// instance settings; the private key normalizes escaped newlines
/// (`_normalize_private_key`). Missing keys fail like
/// `require_github_app_config` (`GithubAppConfigError`, a non-provider
/// error — hence [`GithubError::Transport`], which maps to the base
/// [`GitProviderError::General`], exactly like Python's `_map_error`
/// passthrough).
pub fn github_app_config() -> Result<(String, String), GithubError> {
    let app_id = std::env::var("GITHUB_APP_ID")
        .unwrap_or_default()
        .trim()
        .to_owned();
    let app_slug = std::env::var("GITHUB_APP_SLUG")
        .unwrap_or_default()
        .trim()
        .to_owned();
    let private_key = std::env::var("GITHUB_APP_PRIVATE_KEY")
        .unwrap_or_default()
        .replace("\\r\\n", "\n")
        .replace("\\n", "\n")
        .trim()
        .to_owned();
    let mut missing = Vec::new();
    if app_id.is_empty() {
        missing.push("app_id");
    }
    if app_slug.is_empty() {
        missing.push("app_slug");
    }
    if private_key.is_empty() {
        missing.push("private_key");
    }
    if !missing.is_empty() {
        return Err(GithubError::Transport(format!(
            "GitHub App config missing: {}",
            missing.join(", ")
        )));
    }
    Ok((app_id, private_key))
}

/// App JWT claims (`build_app_jwt`, `github_app_auth.py:72-85`): issued a
/// minute in the past against clock drift, nine-minute expiry.
fn app_jwt_claims(app_id: &str, now_secs: u64) -> serde_json::Map<String, Value> {
    let mut claims = serde_json::Map::new();
    claims.insert("iat".to_owned(), json!(now_secs.saturating_sub(60)));
    claims.insert("exp".to_owned(), json!(now_secs + 9 * 60));
    claims.insert("iss".to_owned(), Value::String(app_id.to_owned()));
    claims
}

/// `build_app_jwt` (`github_app_auth.py:72-85`): RS256-signed app token.
fn build_app_jwt(app_id: &str, private_key: &str, now_secs: u64) -> Result<String, GithubError> {
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(private_key.as_bytes())
        .map_err(|error| GithubError::Transport(error.to_string()))?;
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
        &app_jwt_claims(app_id, now_secs),
        &key,
    )
    .map_err(|error| GithubError::Transport(error.to_string()))
}

/// Minted installation tokens, keyed by installation id with their expiry
/// instant: the process-local half of `installation_token`'s Django-cache
/// memoization (`github_app_auth.py:178-194`). A multi-worker deployment
/// mints once per process instead of once per fleet — harmless (tokens are
/// interchangeable) and converging (each entry expires and refreshes).
static INSTALLATION_TOKENS: LazyLock<Mutex<HashMap<i64, (String, SystemTime)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// `installation_token` (`github_app_auth.py:178-194`): cached mint of a
/// scoped token for one installation — `POST
/// /app/installations/{id}/access_tokens` under the app JWT; the entry
/// lives until sixty seconds before `expires_at` (at least sixty seconds
/// total), defaulting to fifty-five minutes without a parsable expiry.
pub fn installation_token(
    installation_id: i64,
    http: &reqwest::blocking::Client,
) -> Result<String, GithubError> {
    if let Some((token, _)) = INSTALLATION_TOKENS
        .lock()
        .map_err(|error| GithubError::Transport(error.to_string()))?
        .get(&installation_id)
        .filter(|(_, expires_at)| SystemTime::now() < *expires_at)
    {
        return Ok(token.clone());
    }
    let (app_id, private_key) = github_app_config()?;
    let now_secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|error| GithubError::Transport(error.to_string()))?
        .as_secs();
    let jwt = build_app_jwt(&app_id, &private_key, now_secs)?;
    let response = http
        .post(format!(
            "{GITHUB_API_BASE}/app/installations/{installation_id}/access_tokens"
        ))
        .timeout(Duration::from_secs(PROVIDER_TIMEOUT_SECS))
        .header("Authorization", format!("Bearer {jwt}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "pi-dash-github-app")
        .send()
        .map_err(|error| GithubError::Transport(error.to_string()))?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        return Err(GithubError::Transport(format!(
            "GitHub App token mint failed: HTTP {status}: {}",
            response.text().unwrap_or_default()
        )));
    }
    let payload: Value = response_json(response)?;
    let token = payload
        .get("token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            GithubError::Transport("GitHub did not return an installation token".to_owned())
        })?
        .to_owned();
    let ttl = payload
        .get("expires_at")
        .and_then(Value::as_str)
        .and_then(parse_github_datetime)
        .map(|expires_at| {
            let remaining = expires_at
                .signed_duration_since(chrono::Utc::now())
                .num_seconds();
            (remaining - 60).max(60) as u64
        })
        .unwrap_or(55 * 60);
    INSTALLATION_TOKENS
        .lock()
        .map_err(|error| GithubError::Transport(error.to_string()))?
        .insert(
            installation_id,
            (token.clone(), SystemTime::now() + Duration::from_secs(ttl)),
        );
    Ok(token)
}

#[cfg(test)]
pub(crate) fn clear_installation_tokens() {
    if let Ok(mut cache) = INSTALLATION_TOKENS.lock() {
        cache.clear();
    }
}

/// `parse_github_datetime` (`github_app_auth.py:103-106`): `Z` becomes
/// `+00:00`, then `fromisoformat`.
fn parse_github_datetime(value: &str) -> Option<DateTime<Utc>> {
    let normalized = value.replace('Z', "+00:00");
    chrono::DateTime::parse_from_rfc3339(&normalized)
        .ok()
        .map(|fixed| fixed.with_timezone(&Utc))
}

/// Live GitLab transport (`gitlab.py:86-181`): one `requests.request`
/// call per [`GitLabRequest`] — method verbatim, headers applied, query
/// pairs as `params`, form pairs as `data`, redirects NOT followed
/// (`allow_redirects=False`, surfaced so the client maps 3xx itself).
/// Network failures are [`GitProviderError::General`] (in Python the raw
/// `requests` exception propagates past the status mapping the same way);
/// every status, including 3xx/4xx/5xx, returns for the client to map.
#[derive(Debug, Clone, Default)]
pub struct ReqwestGitLabTransport {
    http: Option<reqwest::blocking::Client>,
}

impl ReqwestGitLabTransport {
    fn client(&self) -> Result<reqwest::blocking::Client, GitProviderError> {
        match &self.http {
            Some(client) => Ok(client.clone()),
            None => reqwest::blocking::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|error| GitProviderError::General(error.to_string())),
        }
    }
}

impl GitLabTransport for ReqwestGitLabTransport {
    fn request(&self, request: &GitLabRequest) -> Result<GitLabResponse, GitProviderError> {
        let method: reqwest::Method = request.method.parse().map_err(|_| {
            GitProviderError::General(format!("unsupported HTTP method: {}", request.method))
        })?;
        let mut outgoing = self
            .client()?
            .request(method, &request.url)
            .timeout(Duration::from_secs(request.timeout_secs));
        for (name, value) in &request.headers {
            outgoing = outgoing.header(name, value);
        }
        if !request.query.is_empty() {
            outgoing = outgoing.query(&request.query);
        }
        if !request.form.is_empty() {
            outgoing = outgoing.form(&request.form);
        }
        let response = outgoing
            .send()
            .map_err(|error| GitProviderError::General(error.to_string()))?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_str().unwrap_or("").to_owned()))
            .collect();
        let body = response
            .text()
            .map_err(|error| GitProviderError::General(error.to_string()))?;
        Ok(GitLabResponse {
            status,
            headers,
            body,
        })
    }
}

// ---------------------------------------------------------------------------
// Provider dispatch (mirrors `get_adapter` + the glance-before-try order)
// ---------------------------------------------------------------------------

/// What a provider call failed with. `UnknownProvider` is the `KeyError`
/// from `get_adapter`: it escapes before the guarded `try`, so it records
/// nothing and retries nothing — the task just fails. `Provider` faults
/// classify downstream into the 4xx-record branch vs the retry branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchError {
    UnknownProvider(String),
    Provider(GitProviderError),
}

/// Scan-section fault (`git_sync_task.py:268-278`): provider 4xx faults
/// record `last_sync_error` (+ degrade the account) and ack; anything else
/// records and takes the `self.retry(countdown)` path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanFault {
    Recordable(GitProviderError),
    Unexpected(String),
}

impl ScanFault {
    /// Classify a dispatch failure: auth/permission/not-found record;
    /// unknown providers, transport faults and every other error retry.
    /// (Python: `except (Auth, Permission, NotFound)` first, then
    /// `except Exception`.)
    pub fn classify(error: DispatchError) -> Self {
        match error {
            DispatchError::Provider(
                inner @ (GitProviderError::Auth(_)
                | GitProviderError::Permission(_)
                | GitProviderError::NotFound(_)),
            ) => ScanFault::Recordable(inner),
            DispatchError::Provider(GitProviderError::General(message)) => {
                ScanFault::Unexpected(message)
            }
            DispatchError::UnknownProvider(message) => {
                ScanFault::Unexpected(format!("Unsupported Git provider: {message}"))
            }
        }
    }
}

/// Live provider calls for the scan handlers: the shared Fernet
/// [`Keyring`] (PAT decryption, like `decrypt_data` over
/// `settings.SECRET_KEY`) plus the two blocking transports. Constructed
/// once at registration ([`LiveProviders::from_env`]) and cloned into
/// every handler (`Keyring` is `Clone`).
#[derive(Debug, Clone)]
pub struct LiveProviders {
    keyring: Keyring,
}

impl LiveProviders {
    pub fn from_env() -> Self {
        Self {
            keyring: Keyring::from_env(),
        }
    }

    fn github(&self) -> GitHubAdapter<ReqwestGithubClient> {
        GitHubAdapter::new(self.keyring.clone())
    }

    /// `adapter.list_open_issues(credential, repository)`
    /// (`git_sync_task.py:247`). Unknown providers fail here — before any
    /// database touch, exactly like the `get_adapter` line preceding the
    /// guarded `try`.
    pub fn list_open_issues(
        &self,
        provider: &str,
        credential: &Value,
        repository: &RemoteRepository,
    ) -> Result<Vec<RemoteIssue>, DispatchError> {
        let key = resolve_adapter_key(provider)
            .map_err(|error| DispatchError::UnknownProvider(error.provider().to_owned()))?;
        match key {
            "github" => self
                .github()
                .list_open_issues(credential, repository)
                .map_err(DispatchError::Provider),
            "gitlab" => {
                let transport = ReqwestGitLabTransport::default();
                let adapter = GitLabAdapter::new(&transport);
                adapter
                    .list_open_issues(credential, repository)
                    .map_err(DispatchError::Provider)
            }
            other => Err(DispatchError::UnknownProvider(other.to_owned())),
        }
    }

    /// `adapter.list_issue_comments(credential, repository, issue_iid)`
    /// (`git_sync_task.py:257`).
    pub fn list_issue_comments(
        &self,
        provider: &str,
        credential: &Value,
        repository: &RemoteRepository,
        issue_iid: &str,
    ) -> Result<Vec<RemoteComment>, DispatchError> {
        let key = resolve_adapter_key(provider)
            .map_err(|error| DispatchError::UnknownProvider(error.provider().to_owned()))?;
        match key {
            "github" => self
                .github()
                .list_issue_comments(credential, repository, issue_iid)
                .map_err(DispatchError::Provider),
            "gitlab" => {
                let transport = ReqwestGitLabTransport::default();
                let adapter = GitLabAdapter::new(&transport);
                adapter
                    .list_issue_comments(credential, repository, issue_iid)
                    .map_err(DispatchError::Provider)
            }
            other => Err(DispatchError::UnknownProvider(other.to_owned())),
        }
    }

    /// `adapter.post_issue_comment(credential, repository, issue_iid, body)`
    /// (`git_sync_task.py:305-310`).
    pub fn post_issue_comment(
        &self,
        provider: &str,
        credential: &Value,
        repository: &RemoteRepository,
        issue_iid: &str,
        body: &str,
    ) -> Result<RemoteComment, DispatchError> {
        let key = resolve_adapter_key(provider)
            .map_err(|error| DispatchError::UnknownProvider(error.provider().to_owned()))?;
        match key {
            "github" => self
                .github()
                .post_issue_comment(credential, repository, issue_iid, body)
                .map_err(DispatchError::Provider),
            "gitlab" => {
                let transport = ReqwestGitLabTransport::default();
                let adapter = GitLabAdapter::new(&transport);
                adapter
                    .post_issue_comment(credential, repository, issue_iid, body)
                    .map_err(DispatchError::Provider)
            }
            other => Err(DispatchError::UnknownProvider(other.to_owned())),
        }
    }
}

// ---------------------------------------------------------------------------
// Scan state + upserts (mirrors the ORM bodies statement-for-statement)
// ---------------------------------------------------------------------------

/// Python truthiness for JSON values (`bool(metadata.get(...))`,
/// `config or {}`, `X or ""`): null/false/0/""/[]/{} are falsy.
pub fn json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(signed) = number.as_i64() {
                signed != 0
            } else if let Some(unsigned) = number.as_u64() {
                unsigned != 0
            } else {
                number.as_f64().is_some_and(|float| float != 0.0)
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Parse an adapter-rendered remote datetime back to UTC
/// (`remote_created_at` / `remote_updated_at` cross the DTO boundary as
/// pre-rendered ISO strings). Accepts the offset shapes plus the naive
/// fallbacks the adapters emit (interpreted as UTC, like Django under
/// `USE_TZ`). `None`/empty stays `None`.
pub fn parse_remote_dt(value: Option<&str>) -> Result<Option<DateTime<Utc>>, String> {
    let raw = match value.filter(|text| !text.is_empty()) {
        None => return Ok(None),
        Some(text) => text,
    };
    if let Ok(parsed) = DateTime::parse_from_rfc3339(raw) {
        return Ok(Some(parsed.with_timezone(&Utc)));
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f") {
        return Ok(Some(naive.and_utc()));
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f") {
        return Ok(Some(naive.and_utc()));
    }
    if let Ok(date) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        if let Some(midnight) = date.and_hms_opt(0, 0, 0) {
            return Ok(Some(midnight.and_utc()));
        }
    }
    Err(format!("bad remote datetime: {raw:?}"))
}

/// One binding's scan closure: the `select_related` row decoded
/// (`git_sync_task.py:229-235`) plus the merged provider credential
/// (`services.account_credential`: `credential_config or {}` with
/// `auth_type`/`host_url` defaults filled in — reused, never forked).
pub struct BindingScan {
    pub binding_id: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub account_id: Uuid,
    pub provider: String,
    pub repository: RemoteRepository,
    pub credential: Value,
}

fn value_or_empty_object(value: Value) -> Value {
    match value {
        Value::Null => json!({}),
        other => other,
    }
}

fn decode_binding_scan(row: sqlx::postgres::PgRow) -> Result<BindingScan, String> {
    let repo_metadata: Value = row
        .try_get("repo_metadata")
        .map_err(|error| format!("decode binding scan: {error}"))?;
    let credential_config: Value = row
        .try_get("account_credential_config")
        .map_err(|error| format!("decode binding scan: {error}"))?;
    let auth_type: String = row
        .try_get("account_auth_type")
        .map_err(|error| format!("decode binding scan: {error}"))?;
    let host_url: String = row
        .try_get("account_host_url")
        .map_err(|error| format!("decode binding scan: {error}"))?;
    Ok(BindingScan {
        binding_id: row
            .try_get("binding_id")
            .map_err(|error| format!("decode binding scan: {error}"))?,
        project_id: row
            .try_get("project_id")
            .map_err(|error| format!("decode binding scan: {error}"))?,
        workspace_id: row
            .try_get("workspace_id")
            .map_err(|error| format!("decode binding scan: {error}"))?,
        actor_id: row
            .try_get("actor_id")
            .map_err(|error| format!("decode binding scan: {error}"))?,
        account_id: row
            .try_get("account_id")
            .map_err(|error| format!("decode binding scan: {error}"))?,
        provider: row
            .try_get("provider")
            .map_err(|error| format!("decode binding scan: {error}"))?,
        repository: RemoteRepository {
            // `_remote_repository` (`git_sync_task.py:42-56`): straight
            // field copy, `metadata or {}`.
            provider: row
                .try_get("provider")
                .map_err(|error| format!("decode binding scan: {error}"))?,
            external_id: row
                .try_get("repo_external_id")
                .map_err(|error| format!("decode binding scan: {error}"))?,
            namespace: row
                .try_get("repo_namespace")
                .map_err(|error| format!("decode binding scan: {error}"))?,
            name: row
                .try_get("repo_name")
                .map_err(|error| format!("decode binding scan: {error}"))?,
            full_name: row
                .try_get("repo_full_name")
                .map_err(|error| format!("decode binding scan: {error}"))?,
            web_url: row
                .try_get("repo_web_url")
                .map_err(|error| format!("decode binding scan: {error}"))?,
            clone_url_http: row
                .try_get("repo_clone_http")
                .map_err(|error| format!("decode binding scan: {error}"))?,
            clone_url_ssh: row
                .try_get("repo_clone_ssh")
                .map_err(|error| format!("decode binding scan: {error}"))?,
            default_branch: row
                .try_get("repo_default_branch")
                .map_err(|error| format!("decode binding scan: {error}"))?,
            is_private: row
                .try_get("repo_is_private")
                .map_err(|error| format!("decode binding scan: {error}"))?,
            metadata: value_or_empty_object(repo_metadata),
        },
        credential: account_credential(&credential_config, &auth_type, &host_url),
    })
}

/// `Issue.objects.create(…)` column list, in physical order: the
/// `Issue.save()` creation path resolves pod/state/sequence/sort-order
/// first (see [`create_issue`]), then writes the full row. Application
/// defaults with no DB fallback (`priority 'none'`, `complexity_score 0`,
/// `sequence/sort` resolved, `is_draft false`, `git_work_branch ''`,
/// `workpad ''`); everything else NULL.
const INSERT_ISSUE_SQL: &str = "INSERT INTO issues (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, parent_id, state_id, point, estimate_point_id, name, description_json, description_html, description_stripped, description_binary, priority, complexity_score, start_date, target_date, sequence_id, sort_order, completed_at, archived_at, is_draft, external_source, external_id, type_id, git_work_branch, workpad, created_via, assigned_pod_id, agent_executor) VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, NULL, $8, NULL, NULL, $9, $10, $11, $12, NULL, 'none', 0, NULL, NULL, $13, $14, $15, NULL, FALSE, $16, $17, NULL, '', '', NULL, $18, NULL)";

/// `Issue.objects.filter(pk).update(…)` (`git_sync_task.py:100-108`): only
/// the named columns — `QuerySet.update` bypasses `save()`, so no state,
/// sequence or description-side-table touch. (Both Python `UPDATE`s
/// collapse into this one statement; the final bytes are identical.)
const UPDATE_ISSUE_SQL: &str = "UPDATE issues SET name = $2, description_html = $3, description_stripped = $4, description_json = $5, workspace_id = $6, created_by_id = $7, updated_by_id = $8, external_source = $9, external_id = $10, updated_at = $11 WHERE id = $1";

/// `GitIssueSync.objects.update_or_create(binding, external_iid, …)`
/// (`git_sync_task.py:134-138`): full-row insert and full-defaults update
/// (plus `updated_at`, which `save()` touches on the update path).
const INSERT_ISSUE_SYNC_SQL: &str = "INSERT INTO git_issue_syncs (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, binding_id, issue_id, provider, external_id, external_iid, web_url, remote_state, remote_created_at, remote_updated_at, metadata) VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)";
const UPDATE_ISSUE_SYNC_SQL: &str = "UPDATE git_issue_syncs SET issue_id = $2, provider = $3, external_id = $4, web_url = $5, remote_state = $6, remote_created_at = $7, remote_updated_at = $8, workspace_id = $9, project_id = $10, created_by_id = $11, updated_by_id = $12, metadata = $13, updated_at = $14 WHERE id = $1";

/// `IssueSequence.objects.create(issue, sequence, project)`
/// (`issue.py:340`): `ProjectBaseModel` audit columns, `deleted false`.
const INSERT_ISSUE_SEQUENCE_SQL: &str = "INSERT INTO issue_sequences (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, issue_id, sequence, deleted) VALUES ($1, $2, $3, NULL, NULL, NULL, $4, $5, $6, $7, FALSE)";

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
const INSERT_ISSUE_COMMENT_SQL: &str = "INSERT INTO issue_comments (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, comment_stripped, comment_json, comment_html, description_id, attachments, labels, issue_id, actor_id, access, external_source, external_id, speaker_type, speaker_label, speaker_agent_run_id, edited_at, parent_id) VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, $9, $10, $11, '{}', '{}', $12, $13, 'INTERNAL', $14, $15, 'human', '', NULL, NULL, NULL)";

/// Existing mirror comment refresh (`issue.py:606-646`): the render
/// triple plus audit restamp and `updated_at` (full `save()`); the
/// description side-table updates only when a tracked field changed.
const UPDATE_ISSUE_COMMENT_SQL: &str = "UPDATE issue_comments SET comment_html = $2, comment_stripped = $3, comment_json = $4, workspace_id = $5, project_id = $6, actor_id = $7, created_by_id = $8, updated_by_id = $9, updated_at = $10 WHERE id = $1";
const UPDATE_COMMENT_DESCRIPTION_SQL: &str = "UPDATE descriptions SET description_html = $2, description_stripped = $3, description_json = $4, updated_by_id = $5, updated_at = $6 WHERE id = $1";

/// `GitCommentSync.objects.update_or_create(issue_sync, external_id, …)`
/// (`git_sync_task.py:177-195`).
const INSERT_COMMENT_SYNC_SQL: &str = "INSERT INTO git_comment_syncs (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, workspace_id, issue_sync_id, comment_id, provider, external_id, remote_created_at, remote_updated_at, metadata) VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, $9, $10, $11, $12, $13, $14)";
const UPDATE_COMMENT_SYNC_SQL: &str = "UPDATE git_comment_syncs SET comment_id = $2, provider = $3, remote_created_at = $4, remote_updated_at = $5, workspace_id = $6, project_id = $7, created_by_id = $8, updated_by_id = $9, metadata = $10, updated_at = $11 WHERE id = $1";

/// Binding outcomes: success stamps both columns
/// (`git_sync_task.py:264-266`); faults write only `last_sync_error`
/// (`save(update_fields=[…])` — no `updated_at` touch either way).
const BINDING_SUCCESS_SQL: &str =
    "UPDATE git_repository_bindings SET last_synced_at = $2, last_sync_error = '' WHERE id = $1";
const BINDING_ERROR_SQL: &str =
    "UPDATE git_repository_bindings SET last_sync_error = $2 WHERE id = $1";
const DEGRADE_ACCOUNT_SQL: &str =
    "UPDATE git_provider_accounts SET status = 'degraded', last_check_error = $2 WHERE id = $1";

/// Metadata-only writes (`save(update_fields=["metadata"])`):
/// reconcile flags and the completion guard/error/id keys.
const UPDATE_ISSUE_SYNC_METADATA_SQL: &str =
    "UPDATE git_issue_syncs SET metadata = $2 WHERE id = $1";

/// Create one mirror issue plus its sync row (`_upsert_issue` create arm,
/// `git_sync_task.py:91-97`, with the `Issue.save()` creation path
/// `issue.py:267-340` inlined: pod default, non-triage default state,
/// advisory-locked sequence, sort-order seed, stripped recompute).
#[allow(clippy::too_many_arguments)]
async fn create_issue(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scan: &BindingScan,
    remote: &RemoteIssue,
    name: &str,
    description_html: &str,
    // NOTE: the `_safe_render` stripped value is passed in but `save()`
    // overwrites it (`_ = default_state`'s sibling dead-input ported bug).
    _description_stripped: &str,
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
    let state: Option<(Uuid, String)> = sqlx::query_as(save_state_sql(true))
        .bind(scan.project_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_error)?;
    let state = match state {
        Some(found) => Some(found),
        None => sqlx::query_as(save_state_sql(false))
            .bind(scan.project_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_error)?,
    };
    // `completed_at` stays NULL here: this create call never passes a
    // state, so `Issue.save` takes the `state is None` branch
    // (`issue.py:288-296`) — it resolves the default state but stamps
    // `completed_at` only when the state was already set (`issue.py:302-309`,
    // which this path never reaches).
    let state_id: Option<Uuid> = state.map(|(id, _group)| id);
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
        .bind(scan.workspace_id)
        .bind(state_id)
        .bind(name)
        .bind(json!({}))
        .bind(description_html)
        .bind(stripped)
        .bind(sequence_id)
        .bind(sort_order)
        .bind(completed_at)
        .bind(scan.provider.clone())
        .bind(remote.external_iid.clone())
        .bind(pod_id)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    sqlx::query(INSERT_ISSUE_SEQUENCE_SQL)
        .bind(Uuid::new_v4())
        .bind(*now)
        .bind(*now)
        .bind(scan.project_id)
        .bind(scan.workspace_id)
        .bind(issue_id)
        .bind(sequence_id)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    let sync_id = insert_issue_sync(tx, scan, remote, &issue_id, now).await?;
    Ok((issue_id, sync_id))
}

async fn insert_issue_sync(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scan: &BindingScan,
    remote: &RemoteIssue,
    issue_id: &Uuid,
    now: &DateTime<Utc>,
) -> Result<Uuid, String> {
    let db_error = |error: sqlx::Error| format!("write issue sync: {error}");
    let sync_id = Uuid::new_v4();
    sqlx::query(INSERT_ISSUE_SYNC_SQL)
        .bind(sync_id)
        .bind(*now)
        .bind(*now)
        .bind(scan.actor_id)
        .bind(scan.actor_id)
        .bind(scan.project_id)
        .bind(scan.workspace_id)
        .bind(scan.binding_id)
        .bind(*issue_id)
        .bind(scan.provider.clone())
        .bind(remote.external_id.clone())
        .bind(remote.external_iid.clone())
        .bind(remote.web_url.clone())
        .bind(remote.state.clone())
        .bind(
            parse_remote_dt(remote.created_at.as_deref())
                .map_err(|detail| format!("write issue sync: {detail}"))?,
        )
        .bind(
            parse_remote_dt(remote.updated_at.as_deref())
                .map_err(|detail| format!("write issue sync: {detail}"))?,
        )
        .bind(json!({ "author": remote.author, "remote": remote.metadata }))
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    Ok(sync_id)
}

/// Create-or-update one mirror issue plus its sync row (`_upsert_issue`,
/// `git_sync_task.py:66-139`). Returns the local `(issue_id, sync_id)`.
pub async fn upsert_issue(
    pool: &PgPool,
    scan: &BindingScan,
    remote: &RemoteIssue,
    now: &DateTime<Utc>,
) -> Result<(Uuid, Uuid), String> {
    let db_error = |error: sqlx::Error| format!("upsert mirror issue: {error}");
    let issue_iid = remote.external_iid.clone();
    let name = prefixed_issue_name(&scan.provider, &issue_iid, &remote.title);
    let (description_html, description_stripped) = safe_render(Some(&remote.body));
    let existing: Option<(Uuid, Uuid)> = sqlx::query_as(ISSUE_SYNC_LOOKUP_SQL)
        .bind(scan.binding_id)
        .bind(&issue_iid)
        .fetch_optional(pool)
        .await
        .map_err(db_error)?;
    match existing {
        None => {
            let mut tx = pool.begin().await.map_err(db_error)?;
            let ids = create_issue(
                &mut tx,
                scan,
                remote,
                &name,
                &description_html,
                &description_stripped,
                now,
            )
            .await?;
            tx.commit().await.map_err(db_error)?;
            Ok(ids)
        }
        Some((sync_id, issue_id)) => {
            // `filter(pk).update(…)` (`git_sync_task.py:100-112`): absolute
            // values, no `save()` side effects — then the audit restamp,
            // collapsed into the same statement (identical final bytes).
            sqlx::query(UPDATE_ISSUE_SQL)
                .bind(issue_id)
                .bind(&name)
                .bind(&description_html)
                .bind(&description_stripped)
                .bind(json!({}))
                .bind(scan.workspace_id)
                .bind(scan.actor_id)
                .bind(scan.actor_id)
                .bind(scan.provider.clone())
                .bind(&issue_iid)
                .bind(*now)
                .execute(pool)
                .await
                .map_err(db_error)?;
            let mut tx = pool.begin().await.map_err(db_error)?;
            update_issue_sync(&mut tx, scan, remote, &sync_id, &issue_id, now).await?;
            tx.commit().await.map_err(db_error)?;
            Ok((issue_id, sync_id))
        }
    }
}

async fn update_issue_sync(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scan: &BindingScan,
    remote: &RemoteIssue,
    sync_id: &Uuid,
    issue_id: &Uuid,
    now: &DateTime<Utc>,
) -> Result<(), String> {
    let db_error = |error: sqlx::Error| format!("write issue sync: {error}");
    sqlx::query(UPDATE_ISSUE_SYNC_SQL)
        .bind(*sync_id)
        .bind(*issue_id)
        .bind(scan.provider.clone())
        .bind(remote.external_id.clone())
        .bind(remote.external_iid.clone())
        .bind(remote.web_url.clone())
        .bind(remote.state.clone())
        .bind(
            parse_remote_dt(remote.created_at.as_deref())
                .map_err(|detail| format!("write issue sync: {detail}"))?,
        )
        .bind(
            parse_remote_dt(remote.updated_at.as_deref())
                .map_err(|detail| format!("write issue sync: {detail}"))?,
        )
        .bind(scan.workspace_id)
        .bind(scan.project_id)
        .bind(scan.actor_id)
        .bind(scan.actor_id)
        .bind(json!({ "author": remote.author, "remote": remote.metadata }))
        .bind(*now)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    Ok(())
}

/// Create-or-update one mirrored provider comment (`_upsert_comment`,
/// `git_sync_task.py:142-195`) with the `IssueComment.save` description
/// side-table (`issue.py:598-646`).
pub async fn upsert_comment(
    pool: &PgPool,
    scan: &BindingScan,
    remote: &RemoteComment,
    issue_id: &Uuid,
    sync_id: &Uuid,
    now: &DateTime<Utc>,
) -> Result<(), String> {
    let db_error = |error: sqlx::Error| format!("upsert mirror comment: {error}");
    let provider_name = display_name(&scan.provider);
    let (safe_html, safe_stripped) = safe_render(Some(&remote.body));
    // `comment_html` / `comment_stripped` (`git_sync_task.py:152-153`):
    // the trailing `.strip()` applies to the whole prefixed string — but
    // the value never reaches the database as-is. `update_or_create` calls
    // the full `IssueComment.save()`, which unconditionally recomputes
    // `comment_stripped = strip_tags(comment_html)` (`issue.py:606`,
    // html_processor semantics, no `.strip()`), so the stored column keeps
    // e.g. `"[GitHub] "` for empty bodies. The trimmed expression is a
    // dead intermediate (ported bug, listed in the module docs).
    let comment_html = format!("<p>[{provider_name}] </p>{safe_html}");
    let _trimmed = format!("[{provider_name}] {safe_stripped}")
        .trim()
        .to_owned();
    let comment_stripped = strip_html_text(&comment_html);
    let remote_id = remote.external_id.clone();
    let existing: Option<(Uuid, String, String, Value, Option<Uuid>)> =
        sqlx::query_as(ISSUE_COMMENT_LOOKUP_SQL)
            .bind(*issue_id)
            .bind(scan.provider.clone())
            .bind(&remote_id)
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
                .bind(scan.provider.clone())
                .bind(&remote_id)
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
                .bind(scan.actor_id)
                .bind(*now)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
            // Change-tracked description refresh (`issue.py:629-646`):
            // only the render triple, only when it changed, only with a
            // description row to write to. The comparison runs against the
            // save()-recomputed values (decoded, unstripped), which is what
            // the database holds. The description refresh is a plain
            // `filter().update` (no `Description.save`), so it writes the
            // same recomputed triple — unlike the create path, where
            // `Description.save` overwrites the stripped form with Django
            // semantics (`description.py:22-28`).
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
        .bind(&remote_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?;
    let metadata = json!({
        "author": remote.author,
        "web_url": remote.web_url,
        "remote": remote.metadata,
    });
    match sync_row {
        None => {
            sqlx::query(INSERT_COMMENT_SYNC_SQL)
                .bind(Uuid::new_v4())
                .bind(*now)
                .bind(*now)
                .bind(scan.actor_id)
                .bind(scan.actor_id)
                .bind(scan.project_id)
                .bind(scan.workspace_id)
                .bind(*sync_id)
                .bind(comment_id)
                .bind(scan.provider.clone())
                .bind(&remote_id)
                .bind(
                    parse_remote_dt(remote.created_at.as_deref())
                        .map_err(|detail| format!("upsert mirror comment: {detail}"))?,
                )
                .bind(
                    parse_remote_dt(remote.updated_at.as_deref())
                        .map_err(|detail| format!("upsert mirror comment: {detail}"))?,
                )
                .bind(metadata)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
        }
        Some(row_id) => {
            sqlx::query(UPDATE_COMMENT_SYNC_SQL)
                .bind(row_id)
                .bind(comment_id)
                .bind(scan.provider.clone())
                .bind(
                    parse_remote_dt(remote.created_at.as_deref())
                        .map_err(|detail| format!("upsert mirror comment: {detail}"))?,
                )
                .bind(
                    parse_remote_dt(remote.updated_at.as_deref())
                        .map_err(|detail| format!("upsert mirror comment: {detail}"))?,
                )
                .bind(scan.workspace_id)
                .bind(scan.project_id)
                .bind(scan.actor_id)
                .bind(scan.actor_id)
                .bind(metadata)
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
/// (`_reconcile_upstream_gone`, `git_sync_task.py:198-210`): metadata-only
/// writes, `upstream_gone_at` set to `now` in `isoformat` shape
/// ([`format_eta`] renders exactly that: explicit `+00:00`, microseconds
/// only when nonzero).
pub async fn reconcile_upstream_gone(
    pool: &PgPool,
    binding_id: &Uuid,
    remote_issue_iids: &std::collections::HashSet<String>,
    now: &DateTime<Utc>,
) -> Result<(), String> {
    let db_error = |error: sqlx::Error| format!("reconcile mirrors: {error}");
    let rows: Vec<(Uuid, String, Value)> = sqlx::query_as(RECONCILE_LIST_SQL)
        .bind(*binding_id)
        .fetch_all(pool)
        .await
        .map_err(db_error)?;
    for (sync_id, external_iid, metadata) in rows {
        let mut metadata = metadata;
        let is_present = remote_issue_iids.contains(&external_iid);
        let was_flagged = metadata.get("upstream_gone_at").is_some_and(json_truthy);
        if !is_present && !was_flagged {
            metadata["upstream_gone_at"] = Value::String(format_eta(*now));
            sqlx::query(UPDATE_ISSUE_SYNC_METADATA_SQL)
                .bind(sync_id)
                .bind(metadata)
                .execute(pool)
                .await
                .map_err(db_error)?;
        } else if is_present && was_flagged {
            if let Value::Object(ref mut map) = metadata {
                map.remove("upstream_gone_at");
            }
            sqlx::query(UPDATE_ISSUE_SYNC_METADATA_SQL)
                .bind(sync_id)
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

/// `sync_all_bindings` (`git_sync_task.py:213-222`): beat-driven fan-out —
/// one queue row per enabled binding. Returns the fan-out count (Python
/// returns `None`; the count is the observable parity surface the oracle
/// asserts). A database failure parks as `Fail`: like Celery's task
/// failure it never retries, and the next beat tick refires anyway.
pub async fn sync_all_bindings(pool: &PgPool) -> Result<usize, String> {
    if !git_sync_enabled() {
        return Ok(0);
    }
    let ids: Vec<Uuid> = sqlx::query_scalar(ENABLED_BINDINGS_SQL)
        .fetch_all(pool)
        .await
        .map_err(|error| format!("{SYNC_ALL_BINDINGS_TASK}: enabled scan failed: {error}"))?;
    for id in &ids {
        enqueue(pool, &fanout_job(id))
            .await
            .map_err(|error| format!("{SYNC_ALL_BINDINGS_TASK}: fan-out failed: {error}"))?;
    }
    Ok(ids.len())
}

/// Full-scan sync of one binding (`sync_one_binding`,
/// `git_sync_task.py:223-280`): unknown ids and disabled switches ack
/// silently; provider 4xx faults record and ack; unexpected faults record
/// and retry with [`retry_countdown_secs`]; everything before the guarded
/// region (bad ids, missing rows, unknown providers, setup-query failures)
/// parks as `Fail`, like Celery's unhandled task failure.
pub async fn sync_one_binding(
    pool: &PgPool,
    providers: &LiveProviders,
    binding_id: &str,
    attempts: u32,
) -> Result<Verdict, HandlerError> {
    if !git_sync_enabled() {
        return Ok(Verdict::Ack);
    }
    // `…objects.get(id=binding_id)`: garbage ids raise `ValidationError`
    // (task failure, no retry); misses raise `DoesNotExist` (silent ack).
    let id = match Uuid::parse_str(binding_id) {
        Ok(id) => id,
        Err(_) => {
            return Ok(Verdict::Fail {
                error: format!("{SYNC_ONE_BINDING_TASK}: invalid binding id {binding_id:?}"),
            });
        }
    };
    let row = sqlx::query(BINDING_SCAN_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|error| format!("{SYNC_ONE_BINDING_TASK}: binding lookup failed: {error}"))?;
    let Some(row) = row else {
        return Ok(Verdict::Ack);
    };
    let scan = decode_binding_scan(row)?;
    // `get_adapter` precedes the guarded `try`: unknown providers fail
    // without touching the database.
    if resolve_adapter_key(&scan.provider).is_err() {
        return Ok(Verdict::Fail {
            error: format!(
                "{SYNC_ONE_BINDING_TASK}: Unsupported Git provider: {}",
                scan.provider
            ),
        });
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
                error: format!("{SYNC_ONE_BINDING_TASK}: default state lookup failed: {error}"),
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
                    error: format!("{SYNC_ONE_BINDING_TASK}: default state lookup failed: {error}"),
                });
            }
        },
    };
    let now = Utc::now();
    match run_scan(pool, providers, &scan, &now).await {
        Ok(()) => {
            if let Err(error) = sqlx::query(BINDING_SUCCESS_SQL)
                .bind(scan.binding_id)
                .bind(now)
                .execute(pool)
                .await
            {
                return Ok(Verdict::Fail {
                    error: format!("{SYNC_ONE_BINDING_TASK}: success stamp failed: {error}"),
                });
            }
            Ok(Verdict::Ack)
        }
        Err(ScanFault::Recordable(error)) => {
            let text = binding_error_text(&error);
            if let Err(error) = record_provider_fault(&scan, pool, &text).await {
                return Ok(Verdict::Fail { error });
            }
            Ok(Verdict::Ack)
        }
        Err(ScanFault::Unexpected(message)) => {
            let text = unexpected_error_text(&message);
            if let Err(error) = sqlx::query(BINDING_ERROR_SQL)
                .bind(scan.binding_id)
                .bind(&text)
                .execute(pool)
                .await
            {
                return Ok(Verdict::Fail {
                    error: format!("{SYNC_ONE_BINDING_TASK}: fault record failed: {error}"),
                });
            }
            Ok(Verdict::Retry {
                delay_secs: retry_countdown_secs(attempts),
            })
        }
    }
}

/// Record a 4xx fault and degrade the account (`git_sync_task.py:268-273`).
async fn record_provider_fault(
    scan: &BindingScan,
    pool: &PgPool,
    text: &str,
) -> Result<(), String> {
    sqlx::query(BINDING_ERROR_SQL)
        .bind(scan.binding_id)
        .bind(text)
        .execute(pool)
        .await
        .map_err(|error| format!("{}: fault record failed: {error}", SYNC_ONE_BINDING_TASK))?;
    sqlx::query(DEGRADE_ACCOUNT_SQL)
        .bind(scan.account_id)
        .bind(text)
        .execute(pool)
        .await
        .map_err(|error| format!("{}: account degrade failed: {error}", SYNC_ONE_BINDING_TASK))?;
    Ok(())
}

/// The guarded scan body (`git_sync_task.py:246-266`): list, upsert,
/// comment, reconcile. Provider faults classify into [`ScanFault`];
/// database faults are unexpected (record + retry, like `except
/// Exception`).
async fn run_scan(
    pool: &PgPool,
    providers: &LiveProviders,
    scan: &BindingScan,
    now: &DateTime<Utc>,
) -> Result<(), ScanFault> {
    let remote_issues = {
        let providers = providers.clone();
        let credential = scan.credential.clone();
        let repository = scan.repository.clone();
        let provider = scan.provider.clone();
        tokio::task::spawn_blocking(move || {
            providers.list_open_issues(&provider, &credential, &repository)
        })
        .await
        .map_err(|error| ScanFault::Unexpected(format!("provider call failed: {error}")))?
        .map_err(ScanFault::classify)?
    };
    let mut remote_issue_iids = HashSet::new();
    let mut pairs: Vec<(String, Uuid, Uuid)> = Vec::new();
    for remote_issue in &remote_issues {
        // Empty iids never mirror (`git_sync_task.py:248-249`).
        if remote_issue.external_iid.is_empty() {
            continue;
        }
        let (issue_id, sync_id) = upsert_issue(pool, scan, remote_issue, now)
            .await
            .map_err(ScanFault::Unexpected)?;
        remote_issue_iids.insert(remote_issue.external_iid.clone());
        pairs.push((remote_issue.external_iid.clone(), issue_id, sync_id));
    }
    for (issue_iid, issue_id, sync_id) in &pairs {
        let remote_comments = {
            let providers = providers.clone();
            let credential = scan.credential.clone();
            let repository = scan.repository.clone();
            let provider = scan.provider.clone();
            let issue_iid = issue_iid.clone();
            tokio::task::spawn_blocking(move || {
                providers.list_issue_comments(&provider, &credential, &repository, &issue_iid)
            })
            .await
            .map_err(|error| ScanFault::Unexpected(format!("provider call failed: {error}")))?
            .map_err(ScanFault::classify)?
        };
        for remote_comment in &remote_comments {
            if remote_comment.external_id.is_empty() {
                continue;
            }
            upsert_comment(pool, scan, remote_comment, issue_id, sync_id, now)
                .await
                .map_err(ScanFault::Unexpected)?;
        }
    }
    reconcile_upstream_gone(pool, &scan.binding_id, &remote_issue_iids, now)
        .await
        .map_err(ScanFault::Unexpected)?;
    Ok(())
}

/// One-shot completion comment on the upstream issue
/// (`post_completion_comment`, `git_sync_task.py:281-323`): never retries
/// — every fault is recorded in the sync metadata and acked; only
/// malformed deliveries and record failures park. Unknown ids ack
/// silently; already-posted mirrors short-circuit on the
/// `completion_comment_id` guard.
pub async fn post_completion_comment(
    pool: &PgPool,
    providers: &LiveProviders,
    sync_id: &str,
) -> Result<Verdict, HandlerError> {
    if !git_sync_enabled() {
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
    let provider: String = row
        .try_get("repo_provider")
        .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?;
    if resolve_adapter_key(&provider).is_err() {
        return Ok(Verdict::Fail {
            error: format!("{POST_COMPLETION_COMMENT_TASK}: Unsupported Git provider: {provider}"),
        });
    }
    let credential = {
        let config: Value = row
            .try_get("account_credential_config")
            .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?;
        let auth_type: String = row
            .try_get("account_auth_type")
            .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?;
        let host_url: String = row
            .try_get("account_host_url")
            .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?;
        account_credential(&config, &auth_type, &host_url)
    };
    let repository = RemoteRepository {
        provider: provider.clone(),
        external_id: row
            .try_get("repo_external_id")
            .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?,
        namespace: row
            .try_get("repo_namespace")
            .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?,
        name: row
            .try_get("repo_name")
            .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?,
        full_name: row
            .try_get("repo_full_name")
            .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?,
        web_url: row
            .try_get("repo_web_url")
            .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?,
        clone_url_http: row
            .try_get("repo_clone_http")
            .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?,
        clone_url_ssh: row
            .try_get("repo_clone_ssh")
            .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?,
        default_branch: row
            .try_get("repo_default_branch")
            .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?,
        is_private: row
            .try_get("repo_is_private")
            .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?,
        metadata: row
            .try_get::<Value, _>("repo_metadata")
            .map(value_or_empty_object)
            .unwrap_or_else(|_| json!({})),
    };
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
    let external_iid: String = row
        .try_get("sync_external_iid")
        .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?;
    let sync_row_id: Uuid = row
        .try_get("sync_id")
        .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: decode sync row: {error}"))?;
    let body = completion_body(&pidash_issue_url(
        &base,
        &workspace_slug,
        &project_id,
        &issue_id,
    ));
    let posted = {
        let providers = providers.clone();
        let credential = credential.clone();
        let repository = repository.clone();
        let provider = provider.clone();
        let external_iid = external_iid.clone();
        tokio::task::spawn_blocking(move || {
            providers.post_issue_comment(&provider, &credential, &repository, &external_iid, &body)
        })
        .await
        .map_err(|error| format!("{POST_COMPLETION_COMMENT_TASK}: provider call failed: {error}"))?
    };
    let mut metadata = metadata;
    match posted {
        Ok(comment) => {
            // Success stores the id and clears any earlier error
            // (`git_sync_task.py:321-323`).
            metadata["completion_comment_id"] = Value::String(comment.external_id);
            if let Value::Object(ref mut map) = metadata {
                map.remove("completion_comment_error");
            }
        }
        Err(DispatchError::UnknownProvider(unknown)) => {
            return Ok(Verdict::Fail {
                error: format!(
                    "{POST_COMPLETION_COMMENT_TASK}: Unsupported Git provider: {unknown}"
                ),
            });
        }
        Err(DispatchError::Provider(error)) => {
            // 4xx faults record `Type: message`; anything else records the
            // bare message (`git_sync_task.py:311-319`).
            let text = match &error {
                GitProviderError::General(message) => truncate_chars(message, 500),
                other => completion_error_text(provider_error_label(other), other.message()),
            };
            metadata["completion_comment_error"] = Value::String(text);
        }
    }
    if let Err(error) = sqlx::query(UPDATE_ISSUE_SYNC_METADATA_SQL)
        .bind(sync_row_id)
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
/// invalid signatures). `sync_one_binding` spends the row's retry budget
/// with [`retry_countdown_secs`]; the other two never retry.
pub fn register_git_sync_tasks(registry: &mut Registry, pool: PgPool, providers: LiveProviders) {
    let scan_pool = pool.clone();
    registry.register(
        SYNC_ALL_BINDINGS_TASK,
        Arc::new(move |_job: JobRow| {
            let pool = scan_pool.clone();
            Box::pin(async move {
                match sync_all_bindings(&pool).await {
                    Ok(count) => {
                        if count > 0 {
                            tracing::info!(
                                count,
                                task = SYNC_ALL_BINDINGS_TASK,
                                "git sync: dispatched bindings"
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
    let one_providers = providers.clone();
    registry.register(
        SYNC_ONE_BINDING_TASK,
        Arc::new(move |job: JobRow| {
            let pool = one_pool.clone();
            let providers = one_providers.clone();
            Box::pin(async move {
                let id = match parse_single_id_arg(SYNC_ONE_BINDING_TASK, &job.args, &job.kwargs) {
                    Ok(id) => id,
                    Err(detail) => {
                        tracing::warn!("{detail}");
                        return Ok(Verdict::Ack);
                    }
                };
                let attempts = job.attempts.max(0) as u32;
                sync_one_binding(&pool, &providers, &id, attempts).await
            }) as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>
        }),
    );
    let comment_providers = providers;
    registry.register(
        POST_COMPLETION_COMMENT_TASK,
        Arc::new(move |job: JobRow| {
            let pool = pool.clone();
            let providers = comment_providers.clone();
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
                post_completion_comment(&pool, &providers, &id).await
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
                "pi_dash.bgtasks.git_sync_task.sync_all_bindings",
                "pi_dash.bgtasks.git_sync_task.sync_one_binding",
                "pi_dash.bgtasks.git_sync_task.post_completion_comment",
            ]
        );
        assert_eq!(SYNC_ALL_BINDINGS_TASK, TASK_NAMES[0]);
        assert_eq!(SYNC_ONE_BINDING_TASK, TASK_NAMES[1]);
        assert_eq!(POST_COMPLETION_COMMENT_TASK, TASK_NAMES[2]);
    }

    // Retry budget: `bind=True, max_retries=3` rides the row default.
    #[test]
    fn retry_budget_is_three() {
        assert_eq!(MAX_RETRIES, 3);
        assert_eq!(DEFAULT_MAX_RETRIES, 3);
        assert_eq!(fanout_job(&Uuid::nil()).max_retries, 3);
    }

    // Retry/ETA parity: `countdown=60 * 2**retries`, asserted per attempt.
    #[test]
    fn retry_countdown_matches_celery() {
        assert_eq!([0, 1, 2, 3].map(retry_countdown_secs), [60, 120, 240, 480]);
        // A corrupt row saturates instead of overflowing.
        assert!(retry_countdown_secs(u32::MAX) < u64::MAX);
    }

    // Beat: exactly the legacy entry, 4h crontab, pointing at the fan-out.
    #[test]
    fn beat_entry_matches_fixture() {
        let entries = beat_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, GIT_SYNC_BEAT);
        assert_eq!(entries[0].task, SYNC_ALL_BINDINGS_TASK);
        let expected = crate::schedule::Crontab::parse("0", "*/4", "*", "*", "*").expect("valid");
        assert_eq!(
            entries[0].cadence,
            crate::schedule::Cadence::Crontab(expected)
        );
        let beat = fixture("beat.json");
        assert_eq!(beat["entry"], GIT_SYNC_BEAT);
        assert_eq!(beat["task"], SYNC_ALL_BINDINGS_TASK);
        assert_eq!(beat["schedule"]["minute"], 0);
        assert_eq!(beat["schedule"]["hour"], "*/4");
    }

    // Kill switch: `GIT_SYNC_ENABLED` first, else `GITHUB_SYNC_ENABLED`,
    // default on. One test owns the process env for both variables (no
    // other test in this process touches them).
    #[test]
    fn kill_switch_falls_through_in_order() {
        let saved_git = std::env::var("GIT_SYNC_ENABLED").ok();
        let saved_github = std::env::var("GITHUB_SYNC_ENABLED").ok();
        std::env::remove_var("GIT_SYNC_ENABLED");
        std::env::remove_var("GITHUB_SYNC_ENABLED");
        assert!(git_sync_enabled());
        std::env::set_var("GITHUB_SYNC_ENABLED", "false");
        assert!(!git_sync_enabled());
        std::env::set_var("GITHUB_SYNC_ENABLED", "TRUE");
        assert!(git_sync_enabled());
        std::env::set_var("GIT_SYNC_ENABLED", "false");
        assert!(!git_sync_enabled());
        std::env::set_var("GIT_SYNC_ENABLED", "yes");
        assert!(!git_sync_enabled());
        match saved_git {
            Some(value) => std::env::set_var("GIT_SYNC_ENABLED", value),
            None => std::env::remove_var("GIT_SYNC_ENABLED"),
        }
        match saved_github {
            Some(value) => std::env::set_var("GITHUB_SYNC_ENABLED", value),
            None => std::env::remove_var("GITHUB_SYNC_ENABLED"),
        }
    }

    // Display names: registered adapters resolve case-insensitively;
    // unknown providers fall back to `title()`.
    #[test]
    fn display_names_match_registry() {
        assert_eq!(display_name("github"), "GitHub");
        assert_eq!(display_name("GitHub"), "GitHub");
        assert_eq!(display_name("GITHUB"), "GitHub");
        assert_eq!(display_name("gitlab"), "GitLab");
        assert_eq!(display_name("GitLab"), "GitLab");
        assert_eq!(display_name("bitbucket"), "Bitbucket");
        assert_eq!(display_name("BITBUCKET"), "Bitbucket");
        assert_eq!(display_name("my-provider"), "My-Provider");
        assert_eq!(display_name(""), "");
    }

    // `str.title()` vectors, verified against CPython.
    #[test]
    fn py_title_matches_cpython() {
        for (input, expected) in [
            ("github", "Github"),
            ("BITBUCKET", "Bitbucket"),
            ("my-provider", "My-Provider"),
            ("foo2bar", "Foo2Bar"),
            ("", ""),
            ("a b", "A B"),
        ] {
            assert_eq!(py_title(input), expected, "title({input:?})");
        }
    }

    // Mirror titles carry the `[provider_iid]` prefix, capped at 255 chars
    // (code points, never split bytes).
    #[test]
    fn prefixed_name_truncates_by_chars() {
        assert_eq!(
            prefixed_issue_name("github", "7", "Upstream title"),
            "[github_7] Upstream title"
        );
        let long = format!("{}tail", "é".repeat(300));
        let capped = prefixed_issue_name("github", "7", &long);
        assert_eq!(capped.chars().count(), 255);
        assert!(long.starts_with(&capped[11..]));
    }

    // Golden render vectors, generated from `nh3==0.2.18` (the
    // `base.txt:96` pin) through the exact `_safe_render` pipeline: the
    // HTML column is the sanitizer output, the stripped column is
    // `html_processor.strip_tags` of it (entities decoded — verified
    // against CPython's `MLStripper`, not assumed).
    #[test]
    fn safe_render_matches_nh3_goldens() {
        const GOLDENS: [(Option<&str>, &str, &str); 16] = [
            (None, "<p></p>", ""),
            (Some(""), "<p></p>", ""),
            (Some("Hello"), "<p>Hello</p>", "Hello"),
            (Some("a\nb"), "<p>a<br>b</p>", "ab"),
            (Some("p1\n\np2"), "<p>p1</p><p>p2</p>", "p1p2"),
            (
                Some("<b>bold</b>"),
                "<p>&lt;b&gt;bold&lt;/b&gt;</p>",
                "<b>bold</b>",
            ),
            (
                Some("<script>alert(1)</script>"),
                "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>",
                "<script>alert(1)</script>",
            ),
            (
                Some("[link](http://example.com)"),
                "<p>[link](http://example.com)</p>",
                "[link](http://example.com)",
            ),
            (
                Some("<a href=\"javascript:alert(1)\">x</a>"),
                "<p>&lt;a href=\"javascript:alert(1)\"&gt;x&lt;/a&gt;</p>",
                "<a href=\"javascript:alert(1)\">x</a>",
            ),
            (
                Some("AT&T <Q> \"quoted\" 'apos'"),
                "<p>AT&amp;T &lt;Q&gt; \"quoted\" 'apos'</p>",
                "AT&T <Q> \"quoted\" 'apos'",
            ),
            (
                Some("line1\nline2\n\nline3"),
                "<p>line1<br>line2</p><p>line3</p>",
                "line1line2line3",
            ),
            (
                Some("<p>   </p>"),
                "<p>&lt;p&gt;   &lt;/p&gt;</p>",
                "<p>   </p>",
            ),
            (Some("Title\n====="), "<p>Title<br>=====</p>", "Title====="),
            (
                Some("emoji 😀 done"),
                "<p>emoji 😀 done</p>",
                "emoji 😀 done",
            ),
            (
                Some("<img src=\"https://x.test/a.png\" onerror=\"alert(1)\" alt=\"a\">"),
                "<p>&lt;img src=\"https://x.test/a.png\" onerror=\"alert(1)\" alt=\"a\"&gt;</p>",
                "<img src=\"https://x.test/a.png\" onerror=\"alert(1)\" alt=\"a\">",
            ),
            (
                Some("<mention-component id=\"7\">@bob</mention-component>"),
                "<p>&lt;mention-component id=\"7\"&gt;@bob&lt;/mention-component&gt;</p>",
                "<mention-component id=\"7\">@bob</mention-component>",
            ),
        ];
        for (input, html, stripped) in GOLDENS {
            assert_eq!(
                safe_render(input),
                (html.to_owned(), stripped.to_owned()),
                "input {input:?}"
            );
        }
    }

    // `strip_html_text` ports `html_processor.strip_tags` (`MLStripper`,
    // `convert_charrefs=True`): tags dropped, references decoded. Every
    // vector below was verified against CPython's `HTMLParser` running the
    // repo's exact stripper — including the trailing-space prefix rows the
    // mirror comment writer stores verbatim (`issue.py:606` recomputes
    // without `.strip()`).
    #[test]
    fn strip_html_text_matches_mlstripper() {
        for (input, expected) in [
            ("<p></p>", ""),
            ("<p>Hello</p>", "Hello"),
            ("<p>a<br>b</p>", "ab"),
            ("<p>&lt;b&gt;bold&lt;/b&gt;</p>", "<b>bold</b>"),
            ("<p>AT&amp;T &lt;Q&gt;</p>", "AT&T <Q>"),
            ("<p>A&nbsp;B&#39;C&#x27;D&quot;E</p>", "A\u{a0}B'C'D\"E"),
            ("<p>[GitHub] </p><p></p>", "[GitHub] "),
            ("<p>[GitHub] </p><p>Hello</p>", "[GitHub] Hello"),
            ("a &amp;amp; b", "a &amp; b"),
            ("a &unknown; b", "a &unknown; b"),
            ("a &#65;&#x42; c", "a AB c"),
            ("a &#0; b", "a \u{FFFD} b"),
            ("a &#x110000; c", "a \u{FFFD} c"),
            ("<p>unclosed", "unclosed"),
            ("<p>a</p><!-- c --><p>b</p>", "ab"),
        ] {
            assert_eq!(strip_html_text(input), expected, "strip {input:?}");
        }
    }

    // The transcribed sanitizer config carries the custom tags, the full
    // generic set and the four schemes.
    #[test]
    fn sanitizer_config_matches_content_validator() {
        let tags = sanitizer_tags();
        for tag in [
            "mention-component",
            "label",
            "input",
            "image-component",
            "p",
            "br",
            "a",
        ] {
            assert!(tags.contains(tag), "tag {tag}");
        }
        let generic = sanitizer_generic_attributes();
        for attr in ["class", "id", "data-block-type", "data-emoji-url", "style"] {
            assert!(generic.contains(attr), "generic attr {attr}");
        }
        let per_tag = sanitizer_tag_attributes();
        assert_eq!(per_tag["a"], HashSet::from(["href", "target"]));
        assert!(per_tag["img"].contains("alt"));
        assert!(per_tag["mention-component"].contains("entity_name"));
        assert_eq!(
            sanitizer_url_schemes(),
            HashSet::from(["http", "https", "mailto", "tel"])
        );
    }

    // Error text shapes: labels, truncation widths, multibyte safety.
    #[test]
    fn error_texts_match_python_shapes() {
        assert_eq!(
            binding_error_text(&GitProviderError::Auth("bad".into())),
            "GitProviderAuthError: bad"
        );
        assert_eq!(
            binding_error_text(&GitProviderError::Permission("no".into())),
            "GitProviderPermissionError: no"
        );
        assert_eq!(
            binding_error_text(&GitProviderError::NotFound("gone".into())),
            "GitProviderNotFoundError: gone"
        );
        assert!(binding_error_text(&GitProviderError::Auth("x".into())).len() < 900);
        assert_eq!(truncate_chars(&"é".repeat(1000), 900).chars().count(), 900);
        assert_eq!(
            unexpected_error_text(&"e".repeat(2000)).chars().count(),
            1000
        );
        assert_eq!(
            completion_error_text("GitProviderAuthError", "denied"),
            "GitProviderAuthError: denied"
        );
        assert_eq!(truncate_chars(&"é".repeat(600), 500).chars().count(), 500);
    }

    // Fault classification: 4xx records, everything else retries.
    #[test]
    fn scan_fault_classification_matches_except_order() {
        assert_eq!(
            ScanFault::classify(DispatchError::Provider(GitProviderError::Auth(
                "bad token".into()
            ))),
            ScanFault::Recordable(GitProviderError::Auth("bad token".into()))
        );
        assert_eq!(
            ScanFault::classify(DispatchError::Provider(GitProviderError::Permission(
                "denied".into()
            ))),
            ScanFault::Recordable(GitProviderError::Permission("denied".into()))
        );
        assert_eq!(
            ScanFault::classify(DispatchError::Provider(GitProviderError::NotFound(
                "gone".into()
            ))),
            ScanFault::Recordable(GitProviderError::NotFound("gone".into()))
        );
        assert_eq!(
            ScanFault::classify(DispatchError::Provider(GitProviderError::General(
                "boom".into()
            ))),
            ScanFault::Unexpected("boom".into())
        );
        assert_eq!(
            ScanFault::classify(DispatchError::UnknownProvider("bitbucket".into())),
            ScanFault::Unexpected("Unsupported Git provider: bitbucket".into())
        );
    }

    // Completion URL + body: slash strip, path shape, one-shot text.
    #[test]
    fn completion_url_and_body_match_python() {
        let project = Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap();
        let issue = Uuid::nil();
        assert_eq!(
            pidash_issue_url("https://app.test/", "ws", &project, &issue),
            format!("https://app.test/ws/projects/{project}/issues/{issue}")
        );
        assert_eq!(
            completion_base_url(Some("https://a.test"), Some("https://b.test")),
            Some("https://a.test".to_owned())
        );
        assert_eq!(
            completion_base_url(None, Some("https://b.test")),
            Some("https://b.test".to_owned())
        );
        assert_eq!(completion_base_url(Some(""), Some("")), None);
        assert_eq!(completion_base_url(None, None), None);
        assert_eq!(
            completion_body("https://app.test/ws/projects/1/issues/2"),
            "This issue has been completed in Pi Dash: https://app.test/ws/projects/1/issues/2"
        );
    }

    // Advisory-lock key: `sha256(str(uuid))` head word — pinned against
    // the Python `convert_uuid_to_integer` oracle value.
    #[test]
    fn advisory_lock_key_matches_python() {
        let project = Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap();
        assert_eq!(advisory_lock_key(&project), 8400349069047396436);
    }

    // Python truthiness over JSON (guard + reconcile predicates).
    #[test]
    fn json_truthy_matches_python_bool() {
        assert!(!json_truthy(&json!(null)));
        assert!(!json_truthy(&json!(false)));
        assert!(json_truthy(&json!(true)));
        assert!(!json_truthy(&json!(0)));
        assert!(json_truthy(&json!(1)));
        assert!(!json_truthy(&json!("")));
        assert!(json_truthy(&json!("x")));
        assert!(!json_truthy(&json!([])));
        assert!(!json_truthy(&json!({})));
        assert!(json_truthy(&json!({"a": 1})));
    }

    // Remote datetimes: adapter renders round-trip; naive shapes read as
    // UTC like Django; garbage errors (retry path, never silent NULL).
    #[test]
    fn remote_datetimes_parse_like_django() {
        use chrono::TimeZone;
        assert_eq!(parse_remote_dt(None), Ok(None));
        assert_eq!(parse_remote_dt(Some("")), Ok(None));
        assert_eq!(
            parse_remote_dt(Some("2024-01-02T03:04:05+00:00")),
            Ok(Some(Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap()))
        );
        assert_eq!(
            parse_remote_dt(Some("2024-01-02T03:04:05")),
            Ok(Some(Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap()))
        );
        assert_eq!(
            parse_remote_dt(Some("2024-01-02")),
            Ok(Some(Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap()))
        );
        assert!(parse_remote_dt(Some("not-a-date")).is_err());
    }

    // Wire: fan-out rows carry `args=[str]`, `kwargs={}` and convert to the
    // same Celery v2 body the worker forward path emits.
    #[test]
    fn fanout_wire_matches_delay() {
        let id = Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap();
        let job = fanout_job(&id);
        assert_eq!(job.task, SYNC_ONE_BINDING_TASK);
        assert_eq!(job.args, json!(["12345678-1234-5678-1234-567812345678"]));
        assert_eq!(job.kwargs, json!({}));
        let message = fanout_message(&id);
        assert_eq!(message.task, SYNC_ONE_BINDING_TASK);
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
        // And the forward-path mapping the worker applies to such a row.
        let headers = message.headers();
        assert_eq!(headers["task"], SYNC_ONE_BINDING_TASK);
        assert_eq!(headers["lang"], "py");
    }

    // Arity: exactly `args=[<id>]`, `kwargs={}`; anything else is a
    // malformed delivery (ack-and-warn, never retry).
    #[test]
    fn single_id_arity_matches_celery() {
        let kwargs = json!({});
        assert_eq!(
            parse_single_id_arg(SYNC_ONE_BINDING_TASK, &json!(["abc"]), &kwargs),
            Ok("abc".to_owned())
        );
        assert!(parse_single_id_arg(SYNC_ONE_BINDING_TASK, &json!([]), &kwargs).is_err());
        assert!(parse_single_id_arg(SYNC_ONE_BINDING_TASK, &json!(["a", "b"]), &kwargs).is_err());
        assert!(parse_single_id_arg(SYNC_ONE_BINDING_TASK, &json!([1]), &kwargs).is_err());
        assert!(parse_single_id_arg(SYNC_ONE_BINDING_TASK, &json!([""]), &kwargs).is_err());
        assert!(
            parse_single_id_arg(SYNC_ONE_BINDING_TASK, &json!(["a"]), &json!({"x": 1})).is_err()
        );
    }

    // `Link rel="next"` parsing mirrors the `_next_url` regex arm for arm.
    #[test]
    fn next_link_matches_python_regex() {
        assert_eq!(next_link(""), None);
        assert_eq!(
            next_link(r#"<https://api.test/p2>; rel="next", <https://api.test/p0>; rel="prev""#),
            Some("https://api.test/p2".to_owned())
        );
        assert_eq!(next_link("<https://api.test/p0>; rel=\"prev\""), None);
        assert_eq!(
            next_link("  <https://api.test/p2> ;  rel=\"next\""),
            Some("https://api.test/p2".to_owned())
        );
        assert_eq!(next_link("not-a-link"), None);
    }

    // Installation-token cache: a live entry short-circuits the mint
    // (`installation_token`, `github_app_auth.py:180-182`) with no HTTP.
    #[test]
    fn installation_token_cache_short_circuits() {
        clear_installation_tokens();
        let http = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("client builds");
        INSTALLATION_TOKENS.lock().expect("cache locks").insert(
            4242,
            (
                "cached-token".to_owned(),
                SystemTime::now() + Duration::from_secs(600),
            ),
        );
        assert_eq!(
            installation_token(4242, &http),
            Ok("cached-token".to_owned())
        );
        clear_installation_tokens();
    }

    // App JWT claims: backdated a minute, nine-minute life, app issuer.
    #[test]
    fn app_jwt_claims_match_python() {
        let claims = app_jwt_claims("42", 1_000_000);
        assert_eq!(claims["iat"], json!(999_940));
        assert_eq!(claims["exp"], json!(1_000_540));
        assert_eq!(claims["iss"], json!("42"));
    }

    // Every statement parses as Postgres, and the read statements are
    // SELECTs over the exact tables the ORM touches.
    #[test]
    fn all_statements_parse_as_postgres() {
        for sql in [
            ENABLED_BINDINGS_SQL,
            BINDING_SCAN_SQL,
            default_state_sql(true),
            default_state_sql(false),
            save_state_sql(true),
            save_state_sql(false),
            DEFAULT_POD_SQL,
            MAX_SEQUENCE_SQL,
            MAX_SORT_ORDER_SQL,
            ADVISORY_LOCK_SQL,
            ISSUE_SYNC_LOOKUP_SQL,
            ISSUE_COMMENT_LOOKUP_SQL,
            COMMENT_SYNC_LOOKUP_SQL,
            COMPLETION_LOOKUP_SQL,
            RECONCILE_LIST_SQL,
            INSERT_ISSUE_SQL,
            UPDATE_ISSUE_SQL,
            INSERT_ISSUE_SYNC_SQL,
            UPDATE_ISSUE_SYNC_SQL,
            INSERT_ISSUE_SEQUENCE_SQL,
            INSERT_DESCRIPTION_SQL,
            INSERT_ISSUE_COMMENT_SQL,
            UPDATE_ISSUE_COMMENT_SQL,
            UPDATE_COMMENT_DESCRIPTION_SQL,
            INSERT_COMMENT_SYNC_SQL,
            UPDATE_COMMENT_SYNC_SQL,
            BINDING_SUCCESS_SQL,
            BINDING_ERROR_SQL,
            DEGRADE_ACCOUNT_SQL,
            UPDATE_ISSUE_SYNC_METADATA_SQL,
        ] {
            parse(sql);
        }
        for sql in [
            ENABLED_BINDINGS_SQL,
            BINDING_SCAN_SQL,
            ISSUE_SYNC_LOOKUP_SQL,
            ISSUE_COMMENT_LOOKUP_SQL,
            COMMENT_SYNC_LOOKUP_SQL,
            COMPLETION_LOOKUP_SQL,
            RECONCILE_LIST_SQL,
        ] {
            assert!(is_select(&parse(sql)), "read is a SELECT:\n{sql}");
        }
    }

    // Read predicates mirror the manager scopes and ORM filters.
    #[test]
    fn read_predicates_mirror_orm_scopes() {
        // Enabled set: the kill-column plus the soft-delete scope.
        assert!(ENABLED_BINDINGS_SQL.contains("is_sync_enabled = TRUE"));
        assert!(ENABLED_BINDINGS_SQL.contains("deleted_at IS NULL"));
        // `select_related` joins carry no tombstone filter on the joined
        // tables; only the base row is scoped.
        assert_eq!(BINDING_SCAN_SQL.matches("INNER JOIN").count(), 2);
        assert!(BINDING_SCAN_SQL.contains("b.deleted_at IS NULL"));
        assert!(!BINDING_SCAN_SQL.contains("r.deleted_at"));
        assert!(!BINDING_SCAN_SQL.contains("a.deleted_at"));
        // State queries exclude the triage group and order by sequence.
        for sql in [
            default_state_sql(true),
            default_state_sql(false),
            save_state_sql(true),
            save_state_sql(false),
        ] {
            assert!(sql.contains("\"group\" <> 'triage'"), "{sql}");
            assert!(sql.contains("ORDER BY sequence ASC LIMIT 1"), "{sql}");
        }
        assert!(default_state_sql(true).contains("\"default\" = TRUE"));
        assert!(!default_state_sql(false).contains("\"default\""));
        // Save-time resolution filters the `is_triage` boolean instead.
        assert!(save_state_sql(true).contains("is_triage = FALSE"));
        assert!(!default_state_sql(true).contains("is_triage"));
        // Pod default: project + flag + live rows, oldest first.
        assert!(DEFAULT_POD_SQL.contains("FROM pod"));
        assert!(DEFAULT_POD_SQL.contains("is_default = TRUE"));
        assert!(DEFAULT_POD_SQL.contains("ORDER BY created_at ASC LIMIT 1"));
        // Sort seed matches NULL states with `IS NOT DISTINCT FROM`.
        assert!(MAX_SORT_ORDER_SQL.contains("IS NOT DISTINCT FROM"));
        // Natural upsert keys, all soft-delete scoped.
        assert!(ISSUE_SYNC_LOOKUP_SQL.contains("binding_id = $1 AND external_iid = $2"));
        assert!(ISSUE_COMMENT_LOOKUP_SQL
            .contains("issue_id = $1 AND external_source = $2 AND external_id = $3"));
        assert!(COMMENT_SYNC_LOOKUP_SQL.contains("issue_sync_id = $1 AND external_id = $2"));
        // Completion closure joins all six tables off the scoped sync row.
        assert_eq!(COMPLETION_LOOKUP_SQL.matches("INNER JOIN").count(), 5);
        assert!(COMPLETION_LOOKUP_SQL.contains("workspaces"));
        assert!(COMPLETION_LOOKUP_SQL.contains("s.deleted_at IS NULL"));
    }

    // Write shapes: full-row creates with application defaults; updates
    // touch only the ORM-named columns (no `save()` side effects).
    #[test]
    fn write_shapes_match_orm_calls() {
        // Creates carry the resolved application defaults inline.
        assert!(INSERT_ISSUE_SQL.contains("sequence_id"));
        assert!(INSERT_ISSUE_SQL.contains("sort_order"));
        assert!(INSERT_ISSUE_SQL.contains("assigned_pod_id"));
        assert!(INSERT_ISSUE_SQL.contains("'none', 0"));
        // The issue refresh never touches state/sequence/sort/description
        // tables (QuerySet.update bypasses save()).
        assert!(!UPDATE_ISSUE_SQL.contains("state_id"));
        assert!(!UPDATE_ISSUE_SQL.contains("sequence_id"));
        assert!(!UPDATE_ISSUE_SQL.contains("sort_order"));
        assert!(UPDATE_ISSUE_SQL.contains("updated_at"));
        // Metadata-only writes touch exactly one column.
        assert_eq!(
            UPDATE_ISSUE_SYNC_METADATA_SQL,
            "UPDATE git_issue_syncs SET metadata = $2 WHERE id = $1"
        );
        // Success stamps both outcome columns; faults only the error.
        assert!(BINDING_SUCCESS_SQL.contains("last_synced_at"));
        assert!(BINDING_SUCCESS_SQL.contains("last_sync_error = ''"));
        assert!(!BINDING_ERROR_SQL.contains("last_synced_at"));
        assert!(!BINDING_ERROR_SQL.contains("updated_at"));
        // The 4xx branch degrades the account with the same text.
        assert!(DEGRADE_ACCOUNT_SQL.contains("status = 'degraded'"));
        assert!(DEGRADE_ACCOUNT_SQL.contains("last_check_error"));
        // Comment creates pin the ORM field defaults.
        assert!(INSERT_ISSUE_COMMENT_SQL.contains("'INTERNAL'"));
        assert!(INSERT_ISSUE_COMMENT_SQL.contains("'human'"));
    }

    // Fixture replay (`git_sync_task.before_after.json`): every recorded
    // behavior has a corresponding shape above.
    #[test]
    fn fixture_before_after_is_fully_covered() {
        let fixture = fixture("tasks/git_sync_task.before_after.json");
        // Issue upsert: skip rule, title prefix, render-driven description,
        // ignored default_state, keyed sync row.
        let issue = &fixture["_upsert_issue"];
        assert_eq!(
            issue["skip"],
            "remote_issue.external_iid falsy -> skip (git_sync_task.py:248-249)"
        );
        assert!(prefixed_issue_name("github", "7", "T").starts_with("[github_7] "));
        let (html, stripped) = safe_render(Some("T"));
        assert_eq!((html, stripped), safe_render(Some("T")));
        assert!(issue["default_state_param"]
            .as_str()
            .unwrap()
            .contains("IGNORED"));
        assert!(ISSUE_SYNC_LOOKUP_SQL.contains("external_iid"));
        // Comment upsert: skip rule, display fallback, keyed rows.
        let comment = &fixture["_upsert_comment"];
        assert!(comment["skip"].as_str().unwrap().contains("falsy"));
        assert_eq!(display_name("github"), "GitHub");
        assert!(ISSUE_COMMENT_LOOKUP_SQL.contains("external_source"));
        assert!(COMMENT_SYNC_LOOKUP_SQL.contains("external_id"));
        // Reconcile: flag and unflag keys.
        let reconcile = &fixture["reconcile_upstream_gone"];
        assert!(reconcile["absent_unflagged"]
            .as_str()
            .unwrap()
            .contains("upstream_gone_at"));
        assert!(reconcile["present_flagged"]
            .as_str()
            .unwrap()
            .contains("upstream_gone_at"));
        // Error paths: 4xx records without retry, unexpected retries.
        let errors = &fixture["error_paths"];
        assert!(errors["provider_4xx"]
            .as_str()
            .unwrap()
            .contains("degraded"));
        assert!(errors["unexpected"]
            .as_str()
            .unwrap()
            .contains("max_retries=3"));
        assert_eq!(MAX_RETRIES, 3);
        // Scan: per-issue comments, success stamps, disabled/unknown no-ops.
        let scan = &fixture["scan"];
        assert!(scan["success"].as_str().unwrap().contains("last_synced_at"));
        assert!(BINDING_SCAN_SQL.contains("git_repository_bindings"));
    }

    // Registry: the three owned names register; everything else still
    // forwards to the Python plane via Celery v2.
    #[test]
    fn registry_routes_owned_local_and_rest_to_python() {
        use std::future::Future;
        use std::pin::Pin;
        let mut registry = Registry::new();
        for name in TASK_NAMES {
            let task = name.to_owned();
            registry.register(
                name,
                Arc::new(move |_job: JobRow| {
                    let _ = &task;
                    Box::pin(async move { Ok(Verdict::Ack) })
                        as Pin<Box<dyn Future<Output = _> + Send>>
                }),
            );
        }
        for name in TASK_NAMES {
            assert_eq!(route_for(&registry, name), Route::Local);
            assert!(registry.owns(name));
        }
        assert_eq!(
            route_for(&registry, "pi_dash.bgtasks.github_sync_task.sync_one_repo"),
            Route::PythonOwned
        );
    }
}
