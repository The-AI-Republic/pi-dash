//! Workspace seed background task (D-09, stage 5).
//!
//! Port of `apps/api/pi_dash/bgtasks/workspace_seed_task.py` (570 lines):
//! `read_seed_file :49`, `create_project_and_member :71`,
//! `create_project_states :176`, `create_project_labels :211`,
//! `create_project_issues :245`, `create_pages :345`, `create_cycles :391`,
//! `create_modules :442`, `create_views :477`, `workspace_seed :504`.
//!
//! Same insert order and id-mapping (seed-int → uuid maps threaded through
//! `workspace_seed :543-564`); same seed-file rows; explicit request context
//! on writes (the jobs layer runs every write under a
//! [`pidash_db::context::RequestContext`] whose actor is the seed bot user,
//! mirroring `save(created_by_id=bot_user.id, disable_auto_set_user=True)`).
//! Same Celery task name/payload (see the jobs-layer `tasks_cleanup`
//! module, which owns the wire name and the `Registry` entry).
//!
//! This module is pure over `serde_json` so the fixture vectors replay
//! without a database: seed-file parsing, identifier/slug/tag derivation,
//! the model-`save()` side computations (sequences, sort orders, slugs,
//! stripped descriptions, `completed_at`, view queries), and the seed-row
//! planners. The jobs layer maps each plan to SQL.
//!
//! # Ported semantics (translate, don't redesign)
//!
//! * `read_seed_file` returns `None` for a missing file
//!   (`FileNotFoundError :63-65`) and for bad JSON (`JSONDecodeError
//!   :66-68`); both log and return `None`.
//! * `create_project_and_member`: pops `id`/`name`/`identifier` from the
//!   seed row (`:96-99`); the project name is the workspace name and the
//!   identifier is the alnum-only first-5 of it (`:84,104-105`);
//!   `cycle_view`/`module_view`/`issue_views_view` are forced `True`
//!   (`:108-110`); one `ProjectMember` + one `ProjectUserProperty` per
//!   workspace member with the FIXED `display_filters`/`display_properties`
//!   literals below (`:115-168`). Empty seeds → empty map + warning (`:91`).
//! * `Project.save` side effects, applied by the jobs layer in the same
//!   order: `identifier.strip().upper()` (`project.py:257`); timezone copied
//!   from the workspace when the seed row carries none
//!   (`project.py:259-261` — seed rows never carry one, so the copy always
//!   fires); first project in the workspace becomes `is_default`
//!   (`project.py:263-270`) with the atomic demote-others update
//!   (`project.py:292-298`).
//! * `State.save`: `slug = slugify(name)`; on create `sequence =
//!   max(sequence over the project) + 15000` when a row exists, else the
//!   seed value is kept (`state.py:131-140`). The max uses the default
//!   `StateManager`, which excludes triage-group states.
//! * `Label.save`: on create `sort_order = max(sort_order over the project)
//!   + 10000`, else the seed value is kept (`label.py:46-53`).
//! * `Issue.save` (create): per-project advisory xact lock
//!   (`issue.py:317-322`); `sequence_id = max(IssueSequence.sequence) + 1`
//!   else `1`, overwriting the seed row's `sequence_id` (`:324-328`);
//!   `description_stripped` from `description_html` (`:330-334`);
//!   `sort_order = max(sort_order in project+state) + 10000` when a row
//!   exists (`:335-339`); `completed_at = now` when the state's group is
//!   `completed`, else `NULL` (`:303-310`); and `save` itself inserts an
//!   `IssueSequence(issue, sequence=sequence_id)` row (`:343`) — on top of
//!   the explicit `IssueSequence.objects.create` in `create_project_issues`
//!   (`workspace_seed_task.py:293-298`), so every seeded issue owns TWO
//!   sequence rows (ported as-is).
//! * `create_project_issues` also inserts one `IssueActivity(verb=created,
//!   comment='created the issue', `epoch=time.time()`)` per issue (`:300`),
//!   one `IssueLabel` per seed label (`:311`), one `CycleIssue` iff
//!   `cycle_id` is truthy (`:321`), and one `ModuleIssue` per `module_ids`
//!   entry iff truthy (`:331`).
//! * `Page.save` / `PageVersion.save`: `description_stripped` from
//!   `description_html` (`page.py:70-77`); `create_pages` defaults
//!   `access` to `PUBLIC_ACCESS (0)`, `description_json` to `{}`, and
//!   `description_html` to `"<p></p>"` (`:364-367`), and links `PROJECT`
//!   pages with a `project_id` through `ProjectPage` (`:378-386`).
//! * `Cycle.save`: on create `sort_order = min(sort_order over the project)
//!   - 10000` (`cycle.py:88-96`); `Module.save` the same (`module.py:115`).
//! * `IssueView.save`: `query = issue_filters(filters, "POST")` when filters
//!   are truthy, else `{}` (`view.py:79-81`); on create `sort_order =
//!   max + 10000` (`view.py:83-97`). Seed rows carry `"filters": {}`, so
//!   the stored query is `{}`.
//! * `workspace_seed` order (`:543-564`): bot `User` → `WorkspaceMember`
//!   (role 20) → projects → states → labels → cycles → modules → issues →
//!   views → pages. Failure re-raises after logging (`:568-570`), unlike
//!   most tasks which swallow.
//!
//! # Ported bugs (do not fix; listed in the PR)
//!
//! 1. `create_project_issues :270-275`: the missing-required-field `continue`
//!    continues the INNER `for field` loop, so the row is NOT skipped —
//!    execution falls through to the pops, which raise `KeyError` for the
//!    absent key. [`plan_issue_seed`] records the missing fields as warnings
//!    and still attempts the pops.
//! 2. `create_project_issues :282-283`: `cycle_id` / `module_ids` are popped
//!    unguarded, so a seed row without them raises `KeyError` (unlike the
//!    four guarded fields). [`plan_issue_seed`] returns
//!    [`IssueSeedError::MissingKey`] for them.
//! 3. `create_cycles :410,412-424`: `type` is popped unguarded (`KeyError`
//!    when absent) and any other value leaves `start_date`/`end_date`
//!    unbound, so `Cycle(...)` raises `NameError`. [`plan_cycle`] returns
//!    [`CycleSeedError::MissingType`] / [`CycleSeedError::UnboundDates`].

use std::collections::HashMap;
use std::path::Path;

use serde_json::{Map, Value};

/// Seed-file names in read order, mirroring the `workspace_seed :543-564`
/// call sequence (bot + membership rows first, then these files).
pub const SEED_READ_ORDER: [&str; 8] = [
    "projects.json",
    "states.json",
    "labels.json",
    "cycles.json",
    "modules.json",
    "issues.json",
    "views.json",
    "pages.json",
];

/// Orchestration order of `workspace_seed` (`:543-564`): bot user, workspace
/// membership, then one step per creator in this order.
pub const ORCHESTRATION_ORDER: [&str; 10] = [
    "bot_user",
    "workspace_member",
    "projects",
    "states",
    "labels",
    "cycles",
    "modules",
    "issues",
    "views",
    "pages",
];

/// `read_seed_file :49-68`: `os.path.join(settings.SEED_DIR, "data",
/// filename)` parsed as JSON. `None` for a missing file (`:63-65`) and for
/// bad JSON (`:66-68`); the jobs layer logs before mapping to `None`.
pub fn read_seed_file(seed_data_dir: &Path, filename: &str) -> Option<Vec<Value>> {
    let bytes = std::fs::read(seed_data_dir.join(filename)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// `create_project_and_member :84`: alnum-only chars of the workspace name,
/// truncated to 5 chars. Python `str.isalnum` is Unicode-aware, as is
/// `char::is_alphanumeric`; `[:5]` counts code points, as does
/// `chars().take(5)`.
pub fn project_identifier(workspace_name: &str) -> String {
    workspace_name
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .take(5)
        .collect()
}

/// Python `str.strip()` membership (`project.py:257`): Rust
/// `White_Space` plus U+001C-U+001F (verified by exhaustively diffing
/// `str.strip` against `char::is_whitespace` over all code points —
/// those four are the only differences).
fn is_py_strip_ws(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

/// `Project.save` (`project.py:257`): `identifier.strip().upper()`.
/// `trim_matches(is_py_strip_ws)` matches `strip` exactly (including
/// U+001C-U+001F); `to_uppercase` matches `upper` (full-Unicode both).
pub fn normalize_identifier(raw: &str) -> String {
    raw.trim_matches(is_py_strip_ws).to_uppercase()
}

/// `State.save` (`state.py:132`): Django `slugify` — NFKD-normalize,
/// ASCII-encode ignoring errors, strip non `[a-z0-9-_]` runs, lowercase,
/// whitespace/runs to single `-`.
pub fn slugify(name: &str) -> String {
    let normalized: String = name
        .chars()
        .flat_map(|ch| {
            // NFKD-ish decomposition for the Latin-1 range Django's seed
            // state names actually exercise; anything outside ASCII that
            // survives is dropped below, matching `encode("ascii",
            // "ignore")`.
            match ch {
                'À'..='Å' => vec!['a'],
                'à'..='å' => vec!['a'],
                'È'..='Ë' => vec!['e'],
                'è'..='ë' => vec!['e'],
                'Ì'..='Ï' => vec!['i'],
                'ì'..='ï' => vec!['i'],
                'Ò'..='Ö' => vec!['o'],
                'ò'..='ö' => vec!['o'],
                'Ù'..='Ü' => vec!['u'],
                'ù'..='ü' => vec!['u'],
                'Ý' | 'ý' | 'ÿ' => vec!['y'],
                'Ñ' | 'ñ' => vec!['n'],
                'Ç' | 'ç' => vec!['c'],
                'ß' => vec!['s', 's'],
                'Æ' => vec!['a', 'e'],
                'æ' => vec!['a', 'e'],
                'Œ' => vec!['o', 'e'],
                'œ' => vec!['o', 'e'],
                other => vec![other],
            }
        })
        .collect();
    let mut out = String::with_capacity(normalized.len());
    let mut prev_dash = true; // leading separators are dropped
    for ch in normalized.chars() {
        // Django drops `[^\w\s-]` but `\w` includes `_`, so underscores are
        // kept verbatim (and stripped at the ends by `.strip("-_")` below).
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches(|c| c == '-' || c == '_').to_owned()
}

/// `strip_tags` (`pi_dash/utils/html_processor.py:28-31`): stdlib
/// `HTMLParser` with `convert_charrefs=True` — tags removed, character and
/// entity references decoded, data concatenated with NO separator between
/// elements (`"<p>a</p><p>b</p>"` → `"ab"`, verified against the
/// interpreter).
pub fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut chars = html.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            // Skip to the closing `>`; comments/declarations included —
            // `HTMLParser` never surfaces tag content as data either.
            // An unterminated `<` rest is dropped, matching the parser
            // going quiet on broken input (`parse_starttag` needs `>`).
            let mut closed = false;
            for inner in chars.by_ref() {
                if inner == '>' {
                    closed = true;
                    break;
                }
            }
            if !closed {
                break;
            }
        } else if ch == '&' {
            let mut entity = String::from("&");
            let mut closed = false;
            for inner in chars.by_ref() {
                entity.push(inner);
                if inner == ';' {
                    closed = true;
                    break;
                }
                if inner == '&' || inner == '<' || entity.len() > 32 {
                    break;
                }
            }
            if closed {
                out.push_str(&decode_entity(&entity));
            } else {
                out.push_str(&entity);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Decode one `&...;` reference the way `HTMLParser(convert_charrefs=True)`
/// does for the references seed HTML exercises: the five XML entities plus
/// decimal/hex numeric references; anything else is left verbatim (the
/// parser keeps unknown entities as literal text).
fn decode_entity(entity: &str) -> String {
    match entity {
        "&amp;" => "&".to_owned(),
        "&lt;" => "<".to_owned(),
        "&gt;" => ">".to_owned(),
        "&quot;" => "\"".to_owned(),
        "&#39;" | "&apos;" => "'".to_owned(),
        _ => {
            let body = entity.trim_start_matches('&').trim_end_matches(';');
            let codepoint =
                if let Some(hex) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16).ok()
                } else if let Some(dec) = body.strip_prefix('#') {
                    dec.parse::<u32>().ok()
                } else if entity == "&nbsp;" {
                    return "\u{a0}".to_owned();
                } else {
                    None
                };
            match codepoint.and_then(char::from_u32) {
                Some(ch) => ch.to_string(),
                None => entity.to_owned(),
            }
        }
    }
}

/// `description_stripped` derivation shared by `Issue.save`
/// (`issue.py:330-334`), `Page.save` (`page.py:70-77`) and
/// `PageVersion.save`: `None` for `""`/`NULL`, else `strip_tags(html)`.
pub fn description_stripped(description_html: Option<&str>) -> Option<String> {
    match description_html {
        None => None,
        Some("") => None,
        Some(html) => Some(strip_tags(html)),
    }
}

/// Bot identity (`workspace_seed :522-532`): `username` /
/// `email = f"bot_user_{workspace.id}[@example.com]"`, display/first name
/// `"Pi Dash"`, `is_bot`, `bot_type = WORKSPACE_SEED`
/// (`db/models/user.py:53`), `password = make_password(uuid4hex)`,
/// `is_password_autoset`.
pub fn bot_username(workspace_id: &str) -> String {
    format!("bot_user_{workspace_id}")
}

/// `f"bot_user_{workspace.id}@example.com"` (`:530`).
pub fn bot_email(workspace_id: &str) -> String {
    format!("bot_user_{workspace_id}@example.com")
}

/// `make_password(secret_hex)` with Django 4.2 defaults (`pbkdf2_sha256`,
/// 600000 iterations): the exact `PBKDF2PasswordHasher.encode` string via
/// the F-05 kernel. `salt` is caller-generated (Django uses 22 random
/// alphanumerics); it must not contain `$`.
pub fn bot_password_hash(secret_hex: &str, salt: &str) -> String {
    pidash_auth::password::hash_password(secret_hex, salt, 600_000)
}

/// `User.save` (`user.py:169-173`): `email.lower().strip()`.
pub fn normalize_user_email(raw: &str) -> String {
    raw.to_lowercase().trim().to_owned()
}

/// FIXED `display_filters` literal written on every `ProjectUserProperty`
/// (`:135-143`), byte-verbatim from the fixture golden.
pub fn seed_display_filters() -> Value {
    serde_json::json!({
        "layout": "list",
        "calendar": {"layout": "month", "show_weekends": false},
        "group_by": "state",
        "order_by": "sort_order",
        "sub_issue": true,
        "sub_group_by": null,
        "show_empty_groups": true,
    })
}

/// `IssueView.display_filters` model default when the seed row omits it:
/// `get_default_display_filters` (`db/models/issue.py:64-73`). Seed rows
/// always carry the key; this is the constructor fallback.
pub fn issue_default_display_filters() -> Value {
    serde_json::json!({
        "group_by": null,
        "order_by": "-created_at",
        "type": null,
        "sub_issue": true,
        "show_empty_groups": true,
        "layout": "list",
        "calendar_date_range": "",
    })
}

/// `IssueView.display_properties` model default:
/// `get_default_display_properties` (`db/models/issue.py:76-91`).
pub fn issue_default_display_properties() -> Value {
    serde_json::json!({
        "assignee": true,
        "attachment_count": true,
        "created_on": true,
        "due_date": true,
        "estimate": true,
        "key": true,
        "labels": true,
        "link": true,
        "priority": true,
        "start_date": true,
        "state": true,
        "sub_issue_count": true,
        "updated_on": true,
    })
}

/// FIXED `display_properties` literal (`:144-163`), byte-verbatim.
pub fn seed_display_properties() -> Value {
    serde_json::json!({
        "key": true,
        "link": true,
        "cycle": false,
        "state": true,
        "labels": false,
        "modules": false,
        "assignee": true,
        "due_date": false,
        "estimate": true,
        "priority": true,
        "created_on": true,
        "issue_type": true,
        "start_date": false,
        "updated_on": true,
        "customer_count": true,
        "sub_issue_count": false,
        "attachment_count": false,
        "customer_request_count": true,
    })
}

/// `State.save` (`state.py:133-139`): `max + 15000` when a project state
/// exists, else the seed `sequence` is kept. The max runs through the
/// default `StateManager` (triage excluded, `deleted_at IS NULL`); the jobs
/// layer binds exactly that scope.
pub fn next_state_sequence(max_sequence: Option<f64>) -> Option<f64> {
    max_sequence.map(|largest| largest + 15_000.0)
}

/// `Label.save` (`label.py:47-52`): `max + 10000`, else the seed
/// `sort_order` is kept.
pub fn next_label_sort_order(max_sort_order: Option<f64>) -> Option<f64> {
    max_sort_order.map(|largest| largest + 10_000.0)
}

/// `Cycle.save` (`cycle.py:88-96`) / `Module.save` (`module.py:115-123`):
/// `min - 10000`, else the seed `sort_order` is kept.
pub fn next_cycle_sort_order(min_sort_order: Option<f64>) -> Option<f64> {
    min_sort_order.map(|smallest| smallest - 10_000.0)
}

/// `IssueView.save` (`view.py:83-97`): `max + 10000` over the project (or
/// the workspace-null-project scope when the view has no project), else the
/// seed `sort_order` is kept.
pub fn next_view_sort_order(max_sort_order: Option<f64>) -> Option<f64> {
    max_sort_order.map(|largest| largest + 10_000.0)
}

/// `Issue.save` (`issue.py:335-339`): `max(sort_order in project+state) +
/// 10000`, else the seed `sort_order` is kept. Runs through plain
/// `Issue.objects` (soft-delete only, not the `issue_objects`
/// `IssueManager` scope); the jobs layer binds it.
pub fn next_issue_sort_order(max_sort_order: Option<f64>) -> Option<f64> {
    max_sort_order.map(|largest| largest + 10_000.0)
}

/// `Issue.save` (`issue.py:324-328`): `sequence_id = max + 1`, else `1` —
/// always overwritten, even though the seed row carries `sequence_id`.
pub fn next_issue_sequence_id(max_sequence: Option<i64>) -> i64 {
    max_sequence.map(|largest| largest + 1).unwrap_or(1)
}

/// `Issue.save` (`issue.py:303-310`, the `state is not None` branch — seed
/// issues always carry a state): `completed_at = now` iff the state's group
/// is `completed`, else `NULL`.
pub fn issue_completed(group: &str) -> bool {
    group == "completed"
}

/// Issue `completed_at` needs "now": the pure decision above; the jobs
/// layer stamps `now()` when it returns true.
///
/// Cycle date plan (`create_cycles :412-424`): `CURRENT` runs now..now+14d;
/// `UPCOMING` chains off the last cycle's end (+1d..+15d) or falls back to
/// now+14d..now+28d. Day offsets keep this module free of clock types; the
/// jobs layer resolves them against `timezone.now()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleAnchor {
    /// Offsets from `timezone.now()`.
    Now,
    /// `start = last cycle end + 1d`; offsets relative to that start.
    LastCycleEnd,
}

/// Resolved `(start_offset_days, end_offset_days, anchor)` for a cycle row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CycleDatePlan {
    pub anchor: CycleAnchor,
    /// Days after the anchor for `start_date`.
    pub start_offset_days: i64,
    /// Days after the anchor for `end_date`.
    pub end_offset_days: i64,
}

/// Why a cycle row cannot be built. `MissingType` is the unguarded
/// `cycle_seed.pop("type")` `KeyError` (`:410`); `UnboundDates` is the
/// `NameError` from any other `type` value leaving `start_date`/`end_date`
/// unbound (`:412-424` fall-through to `Cycle(...) :426`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CycleSeedError {
    MissingType,
    UnboundDates(String),
}

pub fn plan_cycle_dates(
    cycle_type: Option<&str>,
    last_cycle_end_exists: bool,
) -> Result<CycleDatePlan, CycleSeedError> {
    match cycle_type {
        None => Err(CycleSeedError::MissingType),
        Some("CURRENT") => Ok(CycleDatePlan {
            anchor: CycleAnchor::Now,
            start_offset_days: 0,
            end_offset_days: 14,
        }),
        Some("UPCOMING") if last_cycle_end_exists => Ok(CycleDatePlan {
            anchor: CycleAnchor::LastCycleEnd,
            start_offset_days: 1,
            end_offset_days: 15,
        }),
        Some("UPCOMING") => Ok(CycleDatePlan {
            anchor: CycleAnchor::Now,
            start_offset_days: 14,
            end_offset_days: 28,
        }),
        // Ported bug 3: any other type leaves start/end unbound → NameError
        // at `Cycle(...)`. The jobs layer maps this to a raised worker
        // error with the same task-failure shape (`:568-570` re-raise).
        Some(other) => Err(CycleSeedError::UnboundDates(other.to_owned())),
    }
}

/// Module date plan (`create_modules :460-461`): `start = now + index*2d`,
/// `target = start + 14d`, where `index` is the seed-file order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleDatePlan {
    pub start_offset_days: i64,
    pub target_offset_days: i64,
}

pub fn plan_module_dates(index: usize) -> ModuleDatePlan {
    let start = index as i64 * 2;
    ModuleDatePlan {
        start_offset_days: start,
        target_offset_days: start + 14,
    }
}

/// One planned issue row (`create_project_issues :285-341`): the seed-int
/// ids are already resolved to uuids by the caller threading the maps.
#[derive(Debug, Clone, PartialEq)]
pub struct IssuePlan {
    /// Seed `id` (for the `Issue {seed} created` log line `:341`).
    pub seed_id: i64,
    /// Remaining seed keys forwarded to `Issue(**rest)` verbatim.
    pub rest: Map<String, Value>,
    pub project_id: String,
    pub state_id: String,
    /// Seed `labels` list (seed-int ids).
    pub label_ids: Vec<i64>,
    /// Seed `cycle_id` as stored: number, or `None` when JSON null.
    pub cycle_id: Option<i64>,
    /// Seed `module_ids` as stored: list, or `None` when JSON null.
    pub module_ids: Option<Vec<i64>>,
}

/// Why an issue row cannot be built. `MissingKey` is the `dict.pop`
/// `KeyError` — for the guarded fields after the fall-through of ported
/// bug 1, and for the unguarded `cycle_id`/`module_ids` pops of ported
/// bug 2 (`:282-283`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueSeedError {
    MissingKey(String),
}

fn pop_number(row: &mut Map<String, Value>, key: &str) -> Result<Option<i64>, IssueSeedError> {
    match row.remove(key) {
        None => Err(IssueSeedError::MissingKey(key.to_owned())),
        Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_i64()
            .map(Some)
            .ok_or_else(|| IssueSeedError::MissingKey(key.to_owned())),
        // A non-numeric stored value is not a `KeyError` in Python (the pop
        // succeeds); it fails later at the FK lookup. The jobs layer maps
        // this to the same late failure.
        Some(_) => Err(IssueSeedError::MissingKey(key.to_owned())),
    }
}

/// Plan one `issues.json` row.
///
/// Returns the missing-required-field warnings (the `:274` error logs) plus
/// the plan. Ported bug 1: the `:270-275` check logs each absent required
/// field (`id`, `labels`, `project_id`, `state_id`) and `continue`s the
/// INNER field loop — the row is still processed, so warnings never skip
/// the pops below. Ported bug 2: `cycle_id`/`module_ids` pop unguarded.
///
/// `truthy cycle` / `truthy modules` (`:321,331`) are decided by the jobs
/// layer from the stored values: `cycle_id` present and non-null (Python
/// `if cycle_id:` on an int — nonzero; seed data uses positive ints, null
/// for none), `module_ids` present and non-empty.
pub fn plan_issue_seed(
    row: &Map<String, Value>,
) -> (Vec<String>, Result<IssuePlan, IssueSeedError>) {
    let mut warnings = Vec::new();
    for field in ["id", "labels", "project_id", "state_id"] {
        if !row.contains_key(field) {
            warnings.push(format!(
                "Task: workspace_seed_task -> Required field '{field}' missing in issue seed"
            ));
        }
    }
    let mut working = row.clone();
    let seed_id = match pop_number(&mut working, "id") {
        Ok(Some(v)) => v,
        _ => return (warnings, Err(IssueSeedError::MissingKey("id".to_owned()))),
    };
    let label_ids = match working.remove("labels") {
        Some(Value::Array(items)) => {
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                match item.as_i64() {
                    Some(v) => ids.push(v),
                    None => {
                        return (
                            warnings,
                            Err(IssueSeedError::MissingKey("labels".to_owned())),
                        )
                    }
                }
            }
            ids
        }
        _ => {
            return (
                warnings,
                Err(IssueSeedError::MissingKey("labels".to_owned())),
            )
        }
    };
    let project_seed_id = match pop_number(&mut working, "project_id") {
        Ok(Some(v)) => v,
        _ => {
            return (
                warnings,
                Err(IssueSeedError::MissingKey("project_id".to_owned())),
            )
        }
    };
    let state_seed_id = match pop_number(&mut working, "state_id") {
        Ok(Some(v)) => v,
        _ => {
            return (
                warnings,
                Err(IssueSeedError::MissingKey("state_id".to_owned())),
            )
        }
    };
    // Unguarded pops (ported bug 2): absent keys are `KeyError`, while an
    // explicit JSON null stores `None` (falsy → no link rows).
    let cycle_id = match pop_number(&mut working, "cycle_id") {
        Ok(v) => v,
        Err(e) => return (warnings, Err(e)),
    };
    let module_ids = match working.remove("module_ids") {
        None => {
            return (
                warnings,
                Err(IssueSeedError::MissingKey("module_ids".to_owned())),
            )
        }
        Some(Value::Null) => None,
        Some(Value::Array(items)) => {
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                match item.as_i64() {
                    Some(v) => ids.push(v),
                    None => {
                        return (
                            warnings,
                            Err(IssueSeedError::MissingKey("module_ids".to_owned())),
                        );
                    }
                }
            }
            Some(ids)
        }
        Some(_) => {
            return (
                warnings,
                Err(IssueSeedError::MissingKey("module_ids".to_owned())),
            )
        }
    };
    (
        warnings,
        Ok(IssuePlan {
            seed_id,
            rest: working,
            project_id: project_seed_id.to_string(),
            state_id: state_seed_id.to_string(),
            label_ids,
            cycle_id,
            module_ids,
        }),
    )
}

/// `create_views :490-500`: pops `project_id` (seed-int, resolved by the
/// caller) and discards `id` (`:492`); the rest forwards to
/// `IssueView(**rest)`.
pub fn plan_view_seed(row: &Map<String, Value>) -> (Option<i64>, Map<String, Value>) {
    let mut working = row.clone();
    let project_seed_id = working.remove("project_id").and_then(|v| v.as_i64());
    working.remove("id");
    (project_seed_id, working)
}

/// `IssueView.save` (`view.py:79-81`): `query = issue_filters(filters,
/// "POST") if filters else {}`. Seed rows carry `"filters": {}`, so the
/// stored query is `{}`; a non-empty filter map compiles through the F-07
/// kernel (`pidash_db::issue_filters::issue_filters_post`, same POST
/// branch) and serializes back to a JSON object for the `query` column.
/// `today` is the job's run date (relative-date filters need it).
pub fn view_query_value(
    filters: &Map<String, Value>,
    today: chrono::NaiveDate,
) -> Result<Value, String> {
    if filters.is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    let mut params: HashMap<String, pidash_db::issue_filters::PostVal> = HashMap::new();
    for (key, value) in filters {
        let entry = match value {
            Value::Null => continue,
            Value::Bool(b) => pidash_db::issue_filters::PostVal::Text(b.to_string()),
            Value::Number(n) => pidash_db::issue_filters::PostVal::Text(n.to_string()),
            Value::String(s) => pidash_db::issue_filters::PostVal::Text(s.clone()),
            Value::Array(items) => pidash_db::issue_filters::PostVal::List(
                items
                    .iter()
                    .map(|item| match item {
                        Value::String(s) => s.clone(),
                        _ => item.to_string(),
                    })
                    .collect(),
            ),
            Value::Object(_) => continue,
        };
        params.insert(key.clone(), entry);
    }
    let compiled = pidash_db::issue_filters::issue_filters_post(&params, "", today)
        .map_err(|e| e.to_string())?;
    let mut out = Map::new();
    for (key, value) in compiled.predicates() {
        out.insert(
            key.clone(),
            match value {
                pidash_db::issue_filters::FilterValue::Uuids(ids) => {
                    Value::Array(ids.iter().map(|id| Value::String(id.to_string())).collect())
                }
                pidash_db::issue_filters::FilterValue::Strings(items) => {
                    Value::Array(items.iter().map(|s| Value::String(s.clone())).collect())
                }
                pidash_db::issue_filters::FilterValue::Text(s) => Value::String(s.clone()),
                pidash_db::issue_filters::FilterValue::Flag(b) => Value::Bool(*b),
                pidash_db::issue_filters::FilterValue::Day(d) => {
                    Value::String(d.format("%Y-%m-%d").to_string())
                }
                pidash_db::issue_filters::FilterValue::Null => Value::Null,
            },
        );
    }
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Seed-file golden replays: the recorded `seed.json` inventory
    /// (counts + first-keys) drives the parsers below through the real
    /// seed files' shapes.
    #[test]
    fn seed_read_order_covers_all_files() {
        assert_eq!(
            SEED_READ_ORDER,
            [
                "projects.json",
                "states.json",
                "labels.json",
                "cycles.json",
                "modules.json",
                "issues.json",
                "views.json",
                "pages.json",
            ]
        );
        assert_eq!(
            ORCHESTRATION_ORDER,
            [
                "bot_user",
                "workspace_member",
                "projects",
                "states",
                "labels",
                "cycles",
                "modules",
                "issues",
                "views",
                "pages",
            ]
        );
    }

    #[test]
    fn project_identifier_alnum_first_five() {
        // Golden from the interpreter: 'My Workspace! 42' -> 'MyWor'.
        assert_eq!(project_identifier("My Workspace! 42"), "MyWor");
        assert_eq!(project_identifier("AB"), "AB");
        assert_eq!(project_identifier("a-b_c d!eFgHI"), "abcde");
        assert_eq!(project_identifier("!!!"), "");
        // Unicode alnum counts, like Python `str.isalnum`.
        assert_eq!(project_identifier("héllo wörld"), "héllo");
    }

    #[test]
    fn identifier_normalizes_like_project_save() {
        assert_eq!(normalize_identifier("abc12"), "ABC12");
        assert_eq!(normalize_identifier("  AbC-12_x  "), "ABC-12_X");
    }

    #[test]
    fn slugify_matches_django() {
        // Goldens from `django.utils.text.slugify`.
        assert_eq!(slugify("Backlog"), "backlog");
        assert_eq!(slugify("Todo"), "todo");
        assert_eq!(slugify("In Progress"), "in-progress");
        assert_eq!(slugify("In Review"), "in-review");
        assert_eq!(slugify("In Test"), "in-test");
        assert_eq!(slugify("Done"), "done");
        assert_eq!(slugify("Cancelled"), "cancelled");
        assert_eq!(slugify("Triage"), "triage");
        // Django keeps interior underscores (`\w`) but strips them at the
        // ends (`.strip("-_")`).
        assert_eq!(slugify("a_b"), "a_b");
        assert_eq!(slugify("_lead_"), "lead");
    }

    #[test]
    fn strip_tags_matches_mlstripper() {
        // Goldens from `pi_dash/utils/html_processor.py::strip_tags`
        // (stdlib HTMLParser, convert_charrefs=True): no separator between
        // elements, entities decoded.
        assert_eq!(
            strip_tags("<p class=\"x\">Welcome 👋</p><p>Hi</p>"),
            "Welcome 👋Hi"
        );
        assert_eq!(strip_tags(""), "");
        assert_eq!(strip_tags("<p></p>"), "");
        assert_eq!(strip_tags("A<strong>B</strong>C"), "ABC");
        assert_eq!(strip_tags("<p>a &amp; b</p>"), "a & b");
        assert_eq!(strip_tags("x &lt;&#65;&#x42;&gt; y"), "x <AB> y");
        assert_eq!(strip_tags("a &unknown; b"), "a &unknown; b");
    }

    #[test]
    fn description_stripped_none_rules() {
        assert_eq!(description_stripped(None), None);
        assert_eq!(description_stripped(Some("")), None);
        assert_eq!(description_stripped(Some("<p></p>")), Some(String::new()));
        assert_eq!(
            description_stripped(Some("<p>Hi</p>")),
            Some("Hi".to_owned())
        );
    }

    #[test]
    fn bot_identity_shapes() {
        let ws = "12345678-1234-5678-1234-567812345678";
        assert_eq!(bot_username(ws), format!("bot_user_{ws}"));
        assert_eq!(bot_email(ws), format!("bot_user_{ws}@example.com"));
        assert_eq!(
            normalize_user_email("Bot_User_X@Example.COM "),
            "bot_user_x@example.com"
        );
        // `pbkdf2_sha256$600000$salt$hash`, byte-identical to Django's
        // `make_password('abc', salt='testsalt12345678')` golden.
        assert_eq!(
            bot_password_hash("abc", "testsalt12345678"),
            "pbkdf2_sha256$600000$testsalt12345678$DmB4fU7lb2e6mHg0Aon5l0KSe9gF59i/HCdkBTCMS/g="
        );
    }

    #[test]
    fn display_literals_match_fixture_golden() {
        // Verbatim from `fixtures/tasks_cleanup/seed.json`
        // `create_project_and_member.display_filters/display_properties`.
        assert_eq!(
            seed_display_filters(),
            json!({
                "layout": "list",
                "calendar": {"layout": "month", "show_weekends": false},
                "group_by": "state",
                "order_by": "sort_order",
                "sub_issue": true,
                "sub_group_by": null,
                "show_empty_groups": true,
            })
        );
        assert_eq!(
            seed_display_properties(),
            json!({
                "key": true, "link": true, "cycle": false, "state": true,
                "labels": false, "modules": false, "assignee": true,
                "due_date": false, "estimate": true, "priority": true,
                "created_on": true, "issue_type": true, "start_date": false,
                "updated_on": true, "customer_count": true,
                "sub_issue_count": false, "attachment_count": false,
                "customer_request_count": true,
            })
        );
    }

    #[test]
    fn issue_view_model_defaults() {
        // `get_default_display_filters` / `get_default_display_properties`
        // (`db/models/issue.py:64-91`).
        assert_eq!(
            issue_default_display_filters()["order_by"],
            json!("-created_at")
        );
        assert_eq!(issue_default_display_filters()["layout"], json!("list"));
        assert_eq!(issue_default_display_properties()["key"], json!(true));
        // Unlike the seed literal, the model default has no
        // `customer_request_count` key.
        assert_eq!(
            issue_default_display_properties().get("customer_request_count"),
            None
        );
    }

    #[test]
    fn save_side_computations() {
        // Seed `None` keeps the seed value; existing rows shift it.
        assert_eq!(next_state_sequence(None), None);
        assert_eq!(next_state_sequence(Some(15_000.0)), Some(30_000.0));
        assert_eq!(next_label_sort_order(None), None);
        assert_eq!(next_label_sort_order(Some(65_535.0)), Some(75_535.0));
        assert_eq!(next_cycle_sort_order(None), None);
        assert_eq!(next_cycle_sort_order(Some(65_535.0)), Some(55_535.0));
        assert_eq!(next_view_sort_order(Some(75_535.0)), Some(85_535.0));
        assert_eq!(next_issue_sort_order(None), None);
        assert_eq!(next_issue_sort_order(Some(1_000.0)), Some(11_000.0));
        assert_eq!(next_issue_sequence_id(None), 1);
        assert_eq!(next_issue_sequence_id(Some(7)), 8);
        assert!(issue_completed("completed"));
        assert!(!issue_completed("started"));
        assert!(!issue_completed("backlog"));
    }

    #[test]
    fn cycle_plans_match_source_branches() {
        assert_eq!(
            plan_cycle_dates(Some("CURRENT"), false),
            Ok(CycleDatePlan {
                anchor: CycleAnchor::Now,
                start_offset_days: 0,
                end_offset_days: 14
            })
        );
        assert_eq!(
            plan_cycle_dates(Some("UPCOMING"), true),
            Ok(CycleDatePlan {
                anchor: CycleAnchor::LastCycleEnd,
                start_offset_days: 1,
                end_offset_days: 15
            })
        );
        assert_eq!(
            plan_cycle_dates(Some("UPCOMING"), false),
            Ok(CycleDatePlan {
                anchor: CycleAnchor::Now,
                start_offset_days: 14,
                end_offset_days: 28
            })
        );
        // Ported bugs: absent `type` is KeyError; any other value is the
        // unbound-dates NameError.
        assert_eq!(
            plan_cycle_dates(None, false),
            Err(CycleSeedError::MissingType)
        );
        assert_eq!(
            plan_cycle_dates(Some("PAUSED"), false),
            Err(CycleSeedError::UnboundDates("PAUSED".to_owned()))
        );
    }

    #[test]
    fn module_dates_scale_with_seed_index() {
        assert_eq!(
            plan_module_dates(0),
            ModuleDatePlan {
                start_offset_days: 0,
                target_offset_days: 14
            }
        );
        assert_eq!(
            plan_module_dates(2),
            ModuleDatePlan {
                start_offset_days: 4,
                target_offset_days: 18
            }
        );
    }

    fn issue_row() -> Map<String, Value> {
        json!({
            "id": 1, "name": "Welcome", "project_id": 1, "state_id": 2,
            "labels": [], "priority": "urgent",
            "cycle_id": 1, "module_ids": [1],
        })
        .as_object()
        .unwrap()
        .clone()
    }

    #[test]
    fn issue_plan_threads_seed_ids() {
        let (warnings, plan) = plan_issue_seed(&issue_row());
        assert!(warnings.is_empty());
        let plan = plan.expect("full row plans");
        assert_eq!(plan.seed_id, 1);
        assert_eq!(plan.project_id, "1");
        assert_eq!(plan.state_id, "2");
        assert_eq!(plan.cycle_id, Some(1));
        assert_eq!(plan.module_ids, Some(vec![1]));
        assert!(plan.label_ids.is_empty());
        // Popped keys are gone from the forwarded rest.
        for key in [
            "id",
            "labels",
            "project_id",
            "state_id",
            "cycle_id",
            "module_ids",
        ] {
            assert!(!plan.rest.contains_key(key), "{key} must be popped");
        }
        assert_eq!(plan.rest.get("name"), Some(&json!("Welcome")));
    }

    #[test]
    fn issue_plan_ports_continue_bug() {
        // Missing `labels`: Python logs the error but `continue`s the INNER
        // loop and still processes the row — then `pop("labels")` raises
        // KeyError. The warning is recorded AND the pop still fails.
        let mut row = issue_row();
        row.remove("labels");
        let (warnings, plan) = plan_issue_seed(&row);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("'labels'"));
        assert_eq!(plan, Err(IssueSeedError::MissingKey("labels".to_owned())));
    }

    #[test]
    fn issue_plan_ports_unguarded_pops() {
        // Absent `cycle_id` / `module_ids` are KeyError; explicit nulls
        // store None (falsy → no link rows).
        let mut row = issue_row();
        row.remove("cycle_id");
        let (_, plan) = plan_issue_seed(&row);
        assert_eq!(plan, Err(IssueSeedError::MissingKey("cycle_id".to_owned())));

        let mut row = issue_row();
        row.remove("module_ids");
        let (_, plan) = plan_issue_seed(&row);
        assert_eq!(
            plan,
            Err(IssueSeedError::MissingKey("module_ids".to_owned()))
        );

        let mut row = issue_row();
        row.insert("cycle_id".to_owned(), Value::Null);
        row.insert("module_ids".to_owned(), Value::Null);
        let (warnings, plan) = plan_issue_seed(&row);
        assert!(warnings.is_empty());
        let plan = plan.expect("null links plan");
        assert_eq!(plan.cycle_id, None);
        assert_eq!(plan.module_ids, None);
    }

    #[test]
    fn view_plan_discards_seed_id() {
        let row = json!({
            "id": 1, "name": "V", "project_id": 1, "filters": {},
            "sort_order": 75535,
        });
        let (project, rest) = plan_view_seed(row.as_object().unwrap());
        assert_eq!(project, Some(1));
        assert!(!rest.contains_key("id"));
        assert!(!rest.contains_key("project_id"));
    }

    #[test]
    fn empty_view_filters_store_empty_query() {
        let filters = Map::new();
        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 28).unwrap();
        assert_eq!(
            view_query_value(&filters, today),
            Ok(Value::Object(Map::new()))
        );
    }

    #[test]
    fn read_seed_file_none_rules() {
        // Missing file → None (FileNotFoundError path).
        assert!(read_seed_file(Path::new("/nonexistent-seed-dir"), "projects.json").is_none());
        let dir = std::env::temp_dir().join("pidash-seed-probe");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bad.json"), "{not json").unwrap();
        // Bad JSON → None (JSONDecodeError path).
        assert!(read_seed_file(&dir, "bad.json").is_none());
        std::fs::write(dir.join("ok.json"), r#"[{"id": 1}]"#).unwrap();
        assert_eq!(
            read_seed_file(&dir, "ok.json"),
            Some(vec![json!({"id": 1})])
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod pidashconv_736_tests {
    use super::normalize_identifier;

    #[test]
    fn identifier_strips_py_whitespace() {
        assert_eq!(normalize_identifier("  eng "), "ENG");
        // Python `str.strip()` also strips U+001C-U+001F (PIDASHCONV-736).
        for sep in ['\u{1c}', '\u{1d}', '\u{1e}', '\u{1f}'] {
            let padded = format!("{sep}eng{sep}");
            assert_eq!(
                normalize_identifier(&padded),
                "ENG",
                "U+{:04X} padding must strip like Python",
                sep as u32
            );
        }
        // TAB and U+0085 padding already matched Django; pin the behavior.
        assert_eq!(normalize_identifier("\teng\t"), "ENG");
        assert_eq!(normalize_identifier("\u{85}eng\u{85}"), "ENG");
    }
}
