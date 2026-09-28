//! D-09 version-task decisions (stage 5, PIDASHCONV-188).
//!
//! Pure logic behind the four version files
//! (`apps/api/pi_dash/bgtasks/issue_version_sync.py`,
//! `issue_description_version_sync.py`, `issue_description_version_task.py`,
//! `page_version_task.py`; drift baseline `01a93e17216faea7bfc156b0f864cbbe420d1c52`).
//! Every function here is pure over injected values so the
//! `rust-api/fixtures/tasks_cleanup/versions.json` vectors replay without a
//! database; the jobs layer drives them against Postgres. SQL text lives in
//! [`pidash_db::tasks_cleanup::version_queries`]; Celery names and handlers
//! live in `pidash-jobs`' `tasks_cleanup` module.
//!
//! Timestamps (`now`) and fresh row ids are caller-provided so tests freeze
//! them (the fixture recorders froze the cutoff at
//! `2026-09-28T06:00:00+00:00`; production passes `Utc::now()` and fresh
//! `Uuid`s, matching Django's client-side id assignment and
//! `timezone.now()` defaults).
//!
//! # Ported semantics (translate, don't redesign)
//!
//! * Owner order is `updated_by`, then `created_by`, then the project-admin
//!   fallback (`role = 20`), in both `get_owner_id`s.
//! * `get_related_data` grouping: `values_list` ordered by `issue_id`,
//!   `groupby` into `{id: [str(...), ...]}`; activities order by
//!   `(issue_id, -created_at)` with the FIRST row per issue kept.
//! * `create_issue_version` returns an UNSAVED row (the caller
//!   `bulk_create`s); missing workspace/project or owner skips with a
//!   warning (`None`); any exception skips (`None`).
//! * `log_issue_version` (`db/models/issue.py:863`) is the single-issue
//!   path: `properties`/`meta` are empty literals, `owned_by` is the passed
//!   user, related sets come from per-issue queries.
//! * Batch windows: `end = min(offset + batch, total)` over
//!   `order_by("created_at")`; `total == 0` and empty batches return early;
//!   `end < total` self-chains with the same `batch_size`/`countdown`.
//! * `schedule_*` calls `.delay(batch_size=int(batch_size), countdown=…)`:
//!   immediate execution of the sync task at offset 0.
//! * `issue_description_version_task` skips when the description is
//!   unchanged AND NOT `is_creating`; `is_creating=True` forces a version.
//! * `track_page_version` skips when `description_html` is unchanged
//!   (a falsy `existing_instance` parses as `{}`); prune deletes at most
//!   ONE oldest row per run when the count exceeds 20.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * `issue_task` compares `str(version.owned_by)` — the related `User`
//!   object rendering `"{username} <{email}>"` — against the user-id string,
//!   so the in-place coalesce branch is effectively dead. Ported exactly:
//!   [`render_owner_display`] + [`issue_task_coalesces`].
//! * `should_update_existing_version` returns bare `None`, not `False`,
//!   when there is no version. [`should_update_existing`] preserves it.
//! * `track_page_version` writes `sub_pages_data={}` always and updates
//!   `updated_at` (not `last_saved_at`) on the update path.
//! * `Page.save`/`PageVersion.save` recompute
//!   `description_stripped = strip_tags(description_html)` (`None` when the
//!   html is empty/`None`); [`derive_stripped`] re-derives it identically.
//! * The live `Issue` model has no `properties`/`meta` columns, so the sync
//!   path's `getattr(issue, "properties", {})` always yields `{}`.
//!
//! Wiring note: the crate root declares `pub mod tasks_cleanup;` and this
//! module's parent declares `pub mod versions;` (foundation changes, per
//! the merged layer-PR precedent); these files are new-files-only.

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{Map, Value};
use std::collections::HashMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Task defaults
// ---------------------------------------------------------------------------

/// `sync_issue_version(batch_size=5000, offset=0, countdown=300)`
/// (`issue_version_sync.py:181`).
pub const ISSUE_SYNC_DEFAULT_BATCH_SIZE: i64 = 5000;
/// Default `offset` of both sync tasks.
pub const SYNC_DEFAULT_OFFSET: i64 = 0;
/// Default `countdown` of both sync tasks (seconds).
pub const SYNC_DEFAULT_COUNTDOWN_SECS: i64 = 300;

/// `sync_issue_description_version(batch_size=5000, offset=0, countdown=300)`
/// (`issue_description_version_sync.py:40`).
pub const DESCRIPTION_SYNC_DEFAULT_BATCH_SIZE: i64 = 5000;

/// `bulk_create(..., batch_size=1000)` chunk size of `sync_issue_version`
/// (`issue_version_sync.py:214`). Single owner: the db query module.
pub use pidash_db::tasks_cleanup::version_queries::ISSUE_VERSION_BULK_BATCH_SIZE;

/// Coalesce/update window (`issue_version_sync.py:50`,
/// `issue_description_version_task.py:16`, `PAGE_VERSION_TASK_TIMEOUT`).
/// Microseconds, not seconds: Python compares the float
/// `total_seconds() <= 600`, so an age of 600.5s does NOT coalesce.
/// Signed: clock-skewed (future) timestamps yield a negative age, which
/// the comparison still accepts.
pub const VERSION_WINDOW_MICROS: i64 = 600_000_000;

/// `track_page_version` keeps at most 20 versions per page
/// (`page_version_task.py:72`).
pub const PAGE_VERSION_KEEP: i64 = 20;

/// Project-admin role literal in both `get_owner_id` fallbacks.
pub const PROJECT_ADMIN_ROLE: i64 = 20;

// ---------------------------------------------------------------------------
// Owner resolution
// ---------------------------------------------------------------------------

/// `get_owner_id` order: `updated_by`, then `created_by`, then the
/// project-admin fallback (`issue_version_sync.py:67-82`,
/// `issue_description_version_sync.py:21-36`). `admin_member` is the
/// `member_id` the admin-fallback query returned (`None` when no row).
pub fn resolve_owner_id(
    updated_by: Option<&str>,
    created_by: Option<&str>,
    admin_member: Option<&str>,
) -> Option<String> {
    if let Some(id) = updated_by {
        return Some(id.to_string());
    }
    if let Some(id) = created_by {
        return Some(id.to_string());
    }
    admin_member.map(str::to_string)
}

/// Render a `User` row the way Python's `str(version.owned_by)` does:
/// `f"{username} <{email}>"` (`db/models/user.py:139-140`). Used ONLY by
/// the `issue_task` coalesce comparison (the bug port).
pub fn render_owner_display(username: &str, email: &str) -> String {
    format!("{username} <{email}>")
}

/// `issue_task` coalesce decision (`issue_version_sync.py:47-51`):
/// same rendered owner AND age within the 600s window. Because the left
/// side renders `"{username} <{email}>"`, this is effectively never true
/// for a real user id — ported as-is.
pub fn issue_task_coalesces(owner_display: &str, user_id: &str, age_micros: i64) -> bool {
    owner_display == user_id && age_micros <= VERSION_WINDOW_MICROS
}

/// Coalesce decision for the id-compared paths
/// (`issue_description_version_task.py:22`,
/// `page_version_task.py:38-42`): `str(owned_by_id) == str(user_id)` and
/// the window. Both sides render ids, so this one can fire.
pub fn version_coalesces(owned_by_id: &str, user_id: &str, age_micros: i64) -> bool {
    owned_by_id == user_id && age_micros <= VERSION_WINDOW_MICROS
}

// ---------------------------------------------------------------------------
// Related-data grouping (`get_related_data`, `issue_version_sync.py:85-128`)
// ---------------------------------------------------------------------------

/// Bulk related data, keyed by issue id rendered as string.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RelatedData {
    pub cycle_issues: HashMap<String, Option<String>>,
    pub assignees: HashMap<String, Vec<String>>,
    pub labels: HashMap<String, Vec<String>>,
    pub modules: HashMap<String, Vec<String>>,
    pub activities: HashMap<String, String>,
}

/// Group pre-ordered `(issue_id, value)` pairs into
/// `{id: [str(value), ...]}` (`groupby` over the `ORDER BY issue_id`
/// record lists at `:91-112`).
pub fn group_pairs(records: &[(String, String)]) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for (issue_id, value) in records {
        out.entry(issue_id.clone()).or_default().push(value.clone());
    }
    out
}

/// Latest activity per issue: input ordered `(issue_id ASC, created_at
/// DESC)`, first row per issue wins (`:115-120`).
pub fn latest_per_issue(records: &[(String, String)]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (issue_id, activity_id) in records {
        out.entry(issue_id.clone())
            .or_insert_with(|| activity_id.clone());
    }
    out
}

// ---------------------------------------------------------------------------
// Batch windows + chaining
// ---------------------------------------------------------------------------

/// One `[offset:end_offset]` slice. `None` means "return early": either the
/// table is empty (`total == 0`) or the slice is empty
/// (`issue_version_sync.py:189-200`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchWindow {
    pub end_offset: i64,
    pub limit: i64,
}

pub fn batch_window(total: i64, batch_size: i64, offset: i64) -> Option<BatchWindow> {
    if total == 0 {
        return None;
    }
    let end_offset = (offset + batch_size).min(total);
    let limit = end_offset - offset;
    if limit <= 0 {
        return None;
    }
    Some(BatchWindow { end_offset, limit })
}

/// `end_offset < total_issues_count` → self-chain (`:217`, `:108`).
pub fn should_chain(end_offset: i64, total: i64) -> bool {
    end_offset < total
}

/// The chained call's kwargs: same `batch_size`/`countdown`,
/// `offset = end_offset`, re-fired with `countdown` (`:218-225`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainSpec {
    pub batch_size: i64,
    pub offset: i64,
    pub countdown_secs: i64,
}

pub fn chain_spec(batch_size: i64, end_offset: i64, countdown_secs: i64) -> ChainSpec {
    ChainSpec {
        batch_size,
        offset: end_offset,
        countdown_secs,
    }
}

/// Split `len` rows into `bulk_create` chunks of `size`
/// (`batch_size=1000` at `issue_version_sync.py:214`).
pub fn chunk_ranges(len: usize, size: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < len {
        let end = (start + size).min(len);
        out.push((start, end));
        start = end;
    }
    out
}

/// `int(batch_size)` in `schedule_issue_version`
/// (`issue_version_sync.py:236`): `int()` truncates floats and maps
/// bools (`True → 1`), and accepts numeric strings; anything else is the
/// `ValueError`/`TypeError` the Python call raises.
pub fn coerce_batch_size(value: &Value, default: i64) -> Result<i64, String> {
    match value {
        Value::Null => Ok(default),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i)
            } else if let Some(f) = n.as_f64() {
                Ok(f.trunc() as i64)
            } else {
                Err(format!("invalid batch_size: {n}"))
            }
        }
        Value::String(s) => s
            .parse::<i64>()
            .map_err(|_| format!("invalid batch_size: {s}")),
        other => Err(format!("invalid batch_size: {other}")),
    }
}

// ---------------------------------------------------------------------------
// Description-version task decisions
// ---------------------------------------------------------------------------

/// `should_update_existing_version`
/// (`issue_description_version_task.py:15-22`): `None` version yields
/// `None` (NOT `False`); otherwise same-owner (str-compared) within the
/// window.
pub fn should_update_existing(
    has_version: bool,
    owned_by_id: &str,
    user_id: &str,
    age_micros: i64,
) -> Option<bool> {
    if !has_version {
        return None;
    }
    Some(version_coalesces(owned_by_id, user_id, age_micros))
}

/// Skip rule (`issue_description_version_task.py:53-54`): unchanged
/// description AND NOT creating → return; `is_creating` forces a version.
pub fn description_task_skips(
    current_html: Option<&str>,
    live_html: Option<&str>,
    is_creating: bool,
) -> bool {
    current_html == live_html && !is_creating
}

// ---------------------------------------------------------------------------
// Page-version task decisions
// ---------------------------------------------------------------------------

/// Skip rule (`page_version_task.py:33`): `current.get("description_html")`
/// (missing key → `None`) equal to the live html → no version. A falsy
/// `existing_instance` parses as `{}`, so its `.get` is also `None`.
pub fn page_task_skips(current_html: Option<&str>, live_html: Option<&str>) -> bool {
    current_html == live_html
}

/// Prune rule (`page_version_task.py:72-74`): strictly more than 20 rows →
/// delete the single oldest.
pub fn prune_needed(version_count: i64) -> bool {
    version_count > PAGE_VERSION_KEEP
}

/// Always-fresh `sub_pages_data` literal (`page_version_task.py:30,69`).
pub fn empty_sub_pages() -> Value {
    Value::Object(Map::new())
}

/// Django `strip_tags` (`django.utils.html.strip_tags`): scan for `<...>`
/// spans and drop them; an unclosed `<` is kept verbatim. Recomputed on
/// every `Page.save`/`PageVersion.save` (`db/models/page.py:70-80,175-181`),
/// so the port re-derives instead of trusting a stale column.
pub fn strip_tags(html: &str) -> String {
    // ASCII-only delimiters, so every slice lands on a char boundary —
    // no UTF-8-boundary panic (cf. the Porting guide semantic traps).
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        rest = &rest[lt..];
        match rest.find('>') {
            Some(rel) => rest = &rest[rel + 1..],
            None => {
                out.push_str(rest);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `description_stripped` derivation shared by both `save()` overrides:
/// `None` when the html is empty or `None`, else `strip_tags(html)`.
pub fn derive_stripped(html: Option<&str>) -> Option<String> {
    match html {
        None => None,
        Some("") => None,
        Some(h) => Some(strip_tags(h)),
    }
}

// ---------------------------------------------------------------------------
// Row builders (INSERT shapes; SQL text lives in the db layer)
// ---------------------------------------------------------------------------

/// The `Issue` columns the version paths read.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueSnapshot {
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    pub id: Uuid,
    pub parent: Option<Uuid>,
    pub state: Option<Uuid>,
    pub estimate_point: Option<Uuid>,
    pub name: String,
    pub priority: String,
    pub start_date: Option<NaiveDate>,
    pub target_date: Option<NaiveDate>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub completed_at: Option<DateTime<Utc>>,
    pub archived_at: Option<NaiveDate>,
    pub is_draft: bool,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub type_id: Option<Uuid>,
}

/// One `issue_versions` row, field-for-field per `ISSUE_VERSION_COLUMNS`.
/// `workspace_id`/`project_id`/`created_by`/`updated_by`/`activity_id` are
/// `Option` because the `log_issue_version` path leaves them unset (NULL);
/// the sync path always fills workspace/project and copies created/updated.
#[derive(Debug, Clone, PartialEq)]
pub struct NewIssueVersion {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    pub id: Uuid,
    pub project_id: Option<Uuid>,
    pub workspace_id: Option<Uuid>,
    pub parent: Option<Uuid>,
    pub state: Option<Uuid>,
    pub estimate_point: Option<Uuid>,
    pub name: String,
    pub priority: String,
    pub start_date: Option<NaiveDate>,
    pub target_date: Option<NaiveDate>,
    pub assignees: Vec<Uuid>,
    pub sequence_id: i32,
    pub labels: Vec<Uuid>,
    pub sort_order: f64,
    pub completed_at: Option<DateTime<Utc>>,
    pub archived_at: Option<NaiveDate>,
    pub is_draft: bool,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub type_id: Option<Uuid>,
    pub cycle: Option<Uuid>,
    pub modules: Vec<Uuid>,
    pub properties: Value,
    pub meta: Value,
    pub last_saved_at: DateTime<Utc>,
    pub issue_id: Uuid,
    pub activity_id: Option<Uuid>,
    pub owned_by_id: Uuid,
}

fn parse_uuid(value: &str) -> Option<Uuid> {
    Uuid::parse_str(value).ok()
}

fn parse_uuid_list(values: &[String]) -> Option<Vec<Uuid>> {
    values.iter().map(|v| parse_uuid(v)).collect()
}

/// `create_issue_version` (`issue_version_sync.py:131-177`): UNSAVED row;
/// `None` when a UUID fails to parse (mirrors the INSERT-time coercion
/// failure the broad `except` turns into `None`). `properties`/`meta` are
/// always `{}`: the live model has no such columns, so
/// `getattr(issue, "properties", {})` takes the default.
pub fn build_issue_version(
    snapshot: &IssueSnapshot,
    owner_id: Uuid,
    related: &RelatedData,
    now: DateTime<Utc>,
    new_id: Uuid,
) -> Option<NewIssueVersion> {
    let key = snapshot.id.to_string();
    let activity_id = match related.activities.get(&key) {
        None => None,
        Some(value) => Some(parse_uuid(value)?),
    };
    let empty: Vec<String> = Vec::new();
    Some(NewIssueVersion {
        created_at: now,
        updated_at: now,
        created_by: snapshot.created_by,
        updated_by: snapshot.updated_by,
        id: new_id,
        project_id: Some(snapshot.project_id),
        workspace_id: Some(snapshot.workspace_id),
        parent: snapshot.parent,
        state: snapshot.state,
        estimate_point: snapshot.estimate_point,
        name: snapshot.name.clone(),
        priority: snapshot.priority.clone(),
        start_date: snapshot.start_date,
        target_date: snapshot.target_date,
        assignees: parse_uuid_list(related.assignees.get(&key).unwrap_or(&empty))?,
        sequence_id: snapshot.sequence_id,
        labels: parse_uuid_list(related.labels.get(&key).unwrap_or(&empty))?,
        sort_order: snapshot.sort_order,
        completed_at: snapshot.completed_at,
        archived_at: snapshot.archived_at,
        is_draft: snapshot.is_draft,
        external_source: snapshot.external_source.clone(),
        external_id: snapshot.external_id.clone(),
        type_id: snapshot.type_id,
        cycle: match related.cycle_issues.get(&key).and_then(|c| c.as_deref()) {
            None => None,
            Some(value) => Some(parse_uuid(value)?),
        },
        modules: parse_uuid_list(related.modules.get(&key).unwrap_or(&empty))?,
        properties: Value::Object(Map::new()),
        meta: Value::Object(Map::new()),
        last_saved_at: now,
        issue_id: snapshot.id,
        activity_id,
        owned_by_id: owner_id,
    })
}

/// `IssueVersion.log_issue_version` (`db/models/issue.py:863-904`):
/// workspace/project/created/updated/activity stay unset; `owned_by` is the
/// passed user (NOT `get_owner_id`); related sets arrive from the
/// per-issue queries; `properties`/`meta` are empty literals.
#[allow(clippy::too_many_arguments)]
pub fn build_log_issue_version(
    snapshot: &IssueSnapshot,
    user_id: Uuid,
    assignees: &[String],
    labels: &[String],
    modules: &[String],
    cycle: Option<&str>,
    now: DateTime<Utc>,
    new_id: Uuid,
) -> Option<NewIssueVersion> {
    Some(NewIssueVersion {
        created_at: now,
        updated_at: now,
        created_by: None,
        updated_by: None,
        id: new_id,
        project_id: None,
        workspace_id: None,
        parent: snapshot.parent,
        state: snapshot.state,
        estimate_point: snapshot.estimate_point,
        name: snapshot.name.clone(),
        priority: snapshot.priority.clone(),
        start_date: snapshot.start_date,
        target_date: snapshot.target_date,
        assignees: parse_uuid_list(assignees)?,
        sequence_id: snapshot.sequence_id,
        labels: parse_uuid_list(labels)?,
        sort_order: snapshot.sort_order,
        completed_at: snapshot.completed_at,
        archived_at: snapshot.archived_at,
        is_draft: snapshot.is_draft,
        external_source: snapshot.external_source.clone(),
        external_id: snapshot.external_id.clone(),
        type_id: snapshot.type_id,
        cycle: match cycle {
            None => None,
            Some(value) => Some(parse_uuid(value)?),
        },
        modules: parse_uuid_list(modules)?,
        properties: Value::Object(Map::new()),
        meta: Value::Object(Map::new()),
        last_saved_at: now,
        issue_id: snapshot.id,
        activity_id: None,
        owned_by_id: user_id,
    })
}

/// No changed keys → no write (`issue_version_sync.py:44`).
pub fn issue_task_writes(changed: &[String]) -> bool {
    !changed.is_empty()
}

/// The `Issue` description columns the description paths read.
#[derive(Debug, Clone, PartialEq)]
pub struct DescriptionSnapshot {
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    pub issue_id: Uuid,
    pub binary: Option<Vec<u8>>,
    pub html: Option<String>,
    pub stripped: Option<String>,
    pub json: Value,
}

/// One `issue_description_versions` row. Both the sync path
/// (`issue_description_version_sync.py:87-101`) and
/// `log_issue_description_version` (`db/models/issue.py:927`) fill the
/// same shape: ids + the four description fields + owner + `last_saved_at`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewDescriptionVersion {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    pub id: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub issue_id: Uuid,
    pub binary: Option<Vec<u8>>,
    pub html: Option<String>,
    pub stripped: Option<String>,
    pub json: Value,
    pub last_saved_at: DateTime<Utc>,
    pub owned_by_id: Uuid,
}

pub fn build_description_version(
    snapshot: &DescriptionSnapshot,
    owner_id: Uuid,
    now: DateTime<Utc>,
    new_id: Uuid,
) -> NewDescriptionVersion {
    NewDescriptionVersion {
        created_at: now,
        updated_at: now,
        created_by: snapshot.created_by,
        updated_by: snapshot.updated_by,
        id: new_id,
        project_id: snapshot.project_id,
        workspace_id: snapshot.workspace_id,
        issue_id: snapshot.issue_id,
        binary: snapshot.binary.clone(),
        html: snapshot.html.clone(),
        stripped: snapshot.stripped.clone(),
        json: snapshot.json.clone(),
        last_saved_at: now,
        owned_by_id: owner_id,
    }
}

/// The `Page` columns `track_page_version` reads.
#[derive(Debug, Clone, PartialEq)]
pub struct PageSnapshot {
    pub workspace_id: Uuid,
    pub id: Uuid,
    pub binary: Option<Vec<u8>>,
    pub html: Option<String>,
    pub json: Value,
}

/// One `page_versions` row (`page_version_task.py:60-70`): the four live
/// description fields, owner = acting user, `stripped` re-derived by the
/// `save()` override, `sub_pages_data` always `{}`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewPageVersion {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub page_id: Uuid,
    pub last_saved_at: DateTime<Utc>,
    pub owned_by_id: Uuid,
    pub binary: Option<Vec<u8>>,
    pub html: Option<String>,
    pub stripped: Option<String>,
    pub json: Value,
    pub sub_pages_data: Value,
}

pub fn build_page_version(
    snapshot: &PageSnapshot,
    user_id: Uuid,
    now: DateTime<Utc>,
    new_id: Uuid,
) -> NewPageVersion {
    NewPageVersion {
        created_at: now,
        updated_at: now,
        id: new_id,
        workspace_id: snapshot.workspace_id,
        page_id: snapshot.id,
        last_saved_at: now,
        owned_by_id: user_id,
        binary: snapshot.binary.clone(),
        html: snapshot.html.clone(),
        stripped: derive_stripped(snapshot.html.as_deref()),
        json: snapshot.json.clone(),
        sub_pages_data: empty_sub_pages(),
    }
}

/// The update-path write (`page_version_task.py:43-57`): four description
/// fields (stripped re-derived) + `sub_pages_data` + **`updated_at`**
/// (not `last_saved_at`), mirroring `update_fields` exactly.
#[derive(Debug, Clone, PartialEq)]
pub struct PageVersionUpdate {
    pub html: Option<String>,
    pub binary: Option<Vec<u8>>,
    pub json: Value,
    pub stripped: Option<String>,
    pub sub_pages_data: Value,
    pub updated_at: DateTime<Utc>,
}

pub fn build_page_version_update(snapshot: &PageSnapshot, now: DateTime<Utc>) -> PageVersionUpdate {
    PageVersionUpdate {
        html: snapshot.html.clone(),
        binary: snapshot.binary.clone(),
        json: snapshot.json.clone(),
        stripped: derive_stripped(snapshot.html.as_deref()),
        sub_pages_data: empty_sub_pages(),
        updated_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn versions_fixture() -> serde_json::Value {
        let body = include_str!("../../../../fixtures/tasks_cleanup/versions.json");
        serde_json::from_str(body).expect("versions.json is valid JSON")
    }

    fn frozen_now() -> DateTime<Utc> {
        // The fixture recorders froze the cutoff here; builders take `now`
        // as a parameter so tests pin it exactly.
        Utc.with_ymd_and_hms(2026, 9, 28, 6, 0, 0).unwrap()
    }

    fn uuid(n: u8) -> Uuid {
        Uuid::parse_str(&format!("11111111-1111-1111-1111-1111111111{n:02}")).unwrap()
    }

    fn snapshot() -> IssueSnapshot {
        IssueSnapshot {
            workspace_id: uuid(1),
            project_id: uuid(2),
            created_by: Some(uuid(5)),
            updated_by: Some(uuid(6)),
            id: uuid(10),
            parent: None,
            state: Some(uuid(20)),
            estimate_point: None,
            name: "Bug".to_string(),
            priority: "high".to_string(),
            start_date: None,
            target_date: None,
            sequence_id: 42,
            sort_order: 100.0,
            completed_at: None,
            archived_at: None,
            is_draft: false,
            external_source: None,
            external_id: None,
            type_id: Some(uuid(21)),
        }
    }

    fn related_for(snap: &IssueSnapshot) -> RelatedData {
        let key = snap.id.to_string();
        RelatedData {
            cycle_issues: HashMap::new(),
            assignees: HashMap::from([(
                key.clone(),
                vec![uuid(30).to_string(), uuid(31).to_string()],
            )]),
            labels: HashMap::new(),
            modules: HashMap::from([(key.clone(), vec![uuid(32).to_string()])]),
            activities: HashMap::from([(key, uuid(33).to_string())]),
        }
    }

    #[test]
    fn sync_defaults_match_fixture() {
        let fx = versions_fixture();
        assert_eq!(fx["sync_issue_version"]["defaults"]["batch_size"], 5000);
        assert_eq!(fx["sync_issue_version"]["defaults"]["offset"], 0);
        assert_eq!(fx["sync_issue_version"]["defaults"]["countdown"], 300);
        assert_eq!(
            fx["sync_issue_description_version"]["defaults"]["batch_size"],
            5000
        );
        assert_eq!(ISSUE_SYNC_DEFAULT_BATCH_SIZE, 5000);
        assert_eq!(SYNC_DEFAULT_OFFSET, 0);
        assert_eq!(SYNC_DEFAULT_COUNTDOWN_SECS, 300);
        assert_eq!(DESCRIPTION_SYNC_DEFAULT_BATCH_SIZE, 5000);
        assert_eq!(ISSUE_VERSION_BULK_BATCH_SIZE, 1000);
    }

    #[test]
    fn owner_resolution_order_matches_goldens() {
        let fx = versions_fixture();
        // `updated_by_wins` golden.
        assert_eq!(
            resolve_owner_id(Some("11"), Some("22"), None).as_deref(),
            Some("11")
        );
        // `created_by_fallback` golden.
        assert_eq!(
            resolve_owner_id(None, Some("22"), None).as_deref(),
            Some("22")
        );
        // Description-sync `updated_by` golden.
        assert_eq!(
            resolve_owner_id(Some("33"), None, None).as_deref(),
            Some("33")
        );
        // No ids anywhere → admin fallback (DB path) or None.
        assert_eq!(
            resolve_owner_id(None, None, Some("admin-9")).as_deref(),
            Some("admin-9")
        );
        assert_eq!(resolve_owner_id(None, None, None), None);
        assert_eq!(fx["get_owner_id"]["updated_by_wins"], 11);
    }

    #[test]
    fn issue_task_coalesce_ports_the_object_str_bug() {
        // `str(User)` renders "{username} <{email}>", never a bare id —
        // so against a real user id the branch is dead (bug ported).
        let display = render_owner_display("alice", "a@example.com");
        assert_eq!(display, "alice <a@example.com>");
        assert!(!issue_task_coalesces(
            &display,
            "550e8400-e29b-41d4-a716-446655440000",
            10
        ));
        // ...but the comparison itself is exact when the strings do match.
        assert!(issue_task_coalesces(&display, &display, 600_000_000));
        assert!(!issue_task_coalesces(&display, &display, 600_000_001));
    }

    #[test]
    fn id_compared_coalesce_matches_goldens() {
        let fx = versions_fixture();
        let goldens = &fx["should_update_existing_version"];
        // `int_ids_str_compared`: both sides stringified before comparing.
        assert!(version_coalesces("5", "5", 10));
        assert_eq!(goldens["recent_same_owner_true"], true);
        assert_eq!(goldens["boundary_600s_true"], true);
        assert!(version_coalesces("u", "u", 600_000_000));
        // Float-boundary fidelity: 600.5s does NOT coalesce, matching
        // `total_seconds() <= 600`.
        assert!(!version_coalesces("u", "u", 600_500_000));
        assert_eq!(goldens["stale_601s_false"], false);
        assert!(!version_coalesces("u", "u", 601_000_000));
        assert_eq!(goldens["other_owner_false"], false);
        assert!(!version_coalesces("a", "b", 10));
    }

    #[test]
    fn should_update_preserves_the_none_trap() {
        let fx = versions_fixture();
        // `'if not version: return' yields None, NOT False`.
        assert_eq!(should_update_existing(false, "u", "u", 0), None);
        assert_eq!(
            fx["should_update_existing_version"]["none_version_returns_none"],
            Value::Null
        );
        assert_eq!(should_update_existing(true, "u", "u", 10), Some(true));
        assert_eq!(should_update_existing(true, "u", "v", 10), Some(false));
    }

    #[test]
    fn empty_diff_means_no_write() {
        // No changes → no write (`issue_version_sync.py:44`).
        assert!(issue_task_writes(&["priority".to_string()]));
        assert!(!issue_task_writes(&[]));
    }

    #[test]
    fn grouping_shapes_match_protocol() {
        // Pre-ordered by issue_id, as the `ORDER BY issue_id` lists arrive.
        let records = vec![
            ("i1".to_string(), "a".to_string()),
            ("i1".to_string(), "b".to_string()),
            ("i2".to_string(), "c".to_string()),
        ];
        let grouped = group_pairs(&records);
        assert_eq!(grouped["i1"], vec!["a".to_string(), "b".to_string()]);
        assert_eq!(grouped["i2"], vec!["c".to_string()]);
        // Activities: (issue ASC, created DESC) → first per issue wins.
        let acts = vec![
            ("i1".to_string(), "new".to_string()),
            ("i1".to_string(), "old".to_string()),
        ];
        assert_eq!(latest_per_issue(&acts)["i1"], "new".to_string());
    }

    #[test]
    fn build_issue_version_maps_fixture_fields() {
        let fx = versions_fixture();
        let fields = &fx["create_issue_version"]["fields"];
        let snap = snapshot();
        let related = related_for(&snap);
        let now = frozen_now();
        let row = build_issue_version(&snap, uuid(6), &related, now, uuid(40)).unwrap();
        // Golden field mapping (fixture ids are non-UUID recording fakes;
        // replay uses same-shape UUID values).
        assert_eq!(row.workspace_id, Some(snap.workspace_id));
        assert_eq!(row.project_id, Some(snap.project_id));
        assert_eq!(row.created_by, snap.created_by);
        assert_eq!(row.updated_by, snap.updated_by);
        assert_eq!(row.owned_by_id, uuid(6));
        assert_eq!(row.activity_id, Some(uuid(33)));
        assert_eq!(row.issue_id, snap.id);
        assert_eq!(row.name, fields["name"].as_str().unwrap());
        assert_eq!(row.priority, fields["priority"].as_str().unwrap());
        assert_eq!(row.sequence_id, 42);
        assert!(!row.is_draft);
        assert_eq!(row.sort_order, 100.0);
        assert_eq!(row.parent, None);
        assert_eq!(row.cycle, None);
        assert_eq!(row.assignees, vec![uuid(30), uuid(31)]);
        assert!(row.labels.is_empty());
        assert_eq!(row.modules, vec![uuid(32)]);
        // `properties`/`meta` are always `{}` (no such Issue columns).
        assert_eq!(row.properties, Value::Object(Map::new()));
        assert_eq!(row.meta, Value::Object(Map::new()));
        assert_eq!(row.last_saved_at, now);
        assert_eq!(
            fx["create_issue_version"]["properties_meta"]["properties"]["p"],
            1
        );
    }

    #[test]
    fn build_issue_version_rejects_bad_uuids() {
        let snap = snapshot();
        let key = snap.id.to_string();
        let mut related = related_for(&snap);
        related
            .assignees
            .insert(key.clone(), vec!["not-a-uuid".to_string()]);
        assert!(build_issue_version(&snap, uuid(6), &related, frozen_now(), uuid(40)).is_none());
        let mut related = related_for(&snap);
        related.activities.insert(key, "act-1".to_string());
        assert!(build_issue_version(&snap, uuid(6), &related, frozen_now(), uuid(40)).is_none());
    }

    #[test]
    fn log_path_leaves_scope_unset_uses_passed_user() {
        let snap = snapshot();
        let row = build_log_issue_version(
            &snap,
            uuid(7),
            &[uuid(30).to_string()],
            &[],
            &[],
            None,
            frozen_now(),
            uuid(41),
        )
        .unwrap();
        assert_eq!(row.workspace_id, None);
        assert_eq!(row.project_id, None);
        assert_eq!(row.created_by, None);
        assert_eq!(row.activity_id, None);
        assert_eq!(row.owned_by_id, uuid(7));
        assert_eq!(row.properties, Value::Object(Map::new()));
        assert_eq!(row.meta, Value::Object(Map::new()));
    }

    #[test]
    fn batch_windows_match_sync_flow() {
        assert_eq!(
            batch_window(12_000, 5000, 0),
            Some(BatchWindow {
                end_offset: 5000,
                limit: 5000
            })
        );
        // Tail batch: `end = min(offset + batch, total)`.
        assert_eq!(
            batch_window(12_000, 5000, 10_000),
            Some(BatchWindow {
                end_offset: 12_000,
                limit: 2000
            })
        );
        // Empty table → early return.
        assert_eq!(batch_window(0, 5000, 0), None);
        // Offset past the end → empty batch → early return.
        assert_eq!(batch_window(100, 5000, 100), None);
        // Chaining: `end < total`.
        assert!(should_chain(5000, 12_000));
        assert!(!should_chain(12_000, 12_000));
        assert_eq!(
            chain_spec(5000, 5000, 300),
            ChainSpec {
                batch_size: 5000,
                offset: 5000,
                countdown_secs: 300
            }
        );
        // `bulk_create(batch_size=1000)` chunking.
        assert_eq!(
            chunk_ranges(2500, 1000),
            vec![(0, 1000), (1000, 2000), (2000, 2500)]
        );
        assert_eq!(chunk_ranges(0, 1000), Vec::new());
    }

    #[test]
    fn batch_size_coercion_matches_int() {
        assert_eq!(coerce_batch_size(&Value::Null, 5000), Ok(5000));
        assert_eq!(coerce_batch_size(&serde_json::json!(10), 5000), Ok(10));
        assert_eq!(coerce_batch_size(&serde_json::json!("25"), 5000), Ok(25));
        assert_eq!(coerce_batch_size(&serde_json::json!(true), 5000), Ok(1));
        assert!(coerce_batch_size(&serde_json::json!("abc"), 5000).is_err());
        assert!(coerce_batch_size(&serde_json::json!({"n": 1}), 5000).is_err());
    }

    #[test]
    fn description_skip_rule_with_creating_flag() {
        // Unchanged + not creating → skip.
        assert!(description_task_skips(
            Some("<p>x</p>"),
            Some("<p>x</p>"),
            false
        ));
        // `is_creating` forces a version even when unchanged.
        assert!(!description_task_skips(
            Some("<p>x</p>"),
            Some("<p>x</p>"),
            true
        ));
        assert!(!description_task_skips(
            Some("<p>a</p>"),
            Some("<p>b</p>"),
            false
        ));
        assert!(!description_task_skips(None, Some("<p>b</p>"), false));
    }

    #[test]
    fn page_skip_rule_missing_key_edge() {
        assert!(page_task_skips(Some("<p>x</p>"), Some("<p>x</p>")));
        assert!(!page_task_skips(Some("<p>a</p>"), Some("<p>b</p>")));
        // Missing key (`.get` → None) vs NULL column: equal → skip.
        assert!(page_task_skips(None, None));
        assert!(!page_task_skips(None, Some("<p></p>")));
    }

    #[test]
    fn strip_tags_matches_django() {
        assert_eq!(strip_tags("<p>hi</p>"), "hi");
        assert_eq!(strip_tags("a<b>c</b>d"), "acd");
        // Unclosed `<` is kept verbatim.
        assert_eq!(strip_tags("a<b"), "a<b");
        assert_eq!(strip_tags("plain"), "plain");
        assert_eq!(strip_tags("<p>héllo wörld</p>"), "héllo wörld");
        assert_eq!(derive_stripped(None), None);
        assert_eq!(derive_stripped(Some("")), None);
        assert_eq!(derive_stripped(Some("<p>x</p>")), Some("x".to_string()));
    }

    #[test]
    fn prune_and_sub_pages_rules() {
        assert!(!prune_needed(20));
        assert!(prune_needed(21));
        assert_eq!(empty_sub_pages(), Value::Object(Map::new()));
    }

    #[test]
    fn build_description_version_copies_four_fields() {
        let snap = DescriptionSnapshot {
            workspace_id: uuid(1),
            project_id: uuid(2),
            created_by: Some(uuid(5)),
            updated_by: None,
            issue_id: uuid(10),
            binary: None,
            html: Some("<p>hi</p>".to_string()),
            stripped: Some("hi".to_string()),
            json: serde_json::json!({"ops": []}),
        };
        let now = frozen_now();
        let row = build_description_version(&snap, uuid(5), now, uuid(42));
        assert_eq!(row.workspace_id, uuid(1));
        assert_eq!(row.issue_id, uuid(10));
        assert_eq!(row.html.as_deref(), Some("<p>hi</p>"));
        assert_eq!(row.json, serde_json::json!({"ops": []}));
        assert_eq!(row.owned_by_id, uuid(5));
        assert_eq!(row.last_saved_at, now);
    }

    #[test]
    fn build_page_version_derives_and_empties() {
        let snap = PageSnapshot {
            workspace_id: uuid(1),
            id: uuid(11),
            binary: None,
            html: Some("<p>hi</p>".to_string()),
            json: serde_json::json!({}),
        };
        let now = frozen_now();
        let row = build_page_version(&snap, uuid(7), now, uuid(43));
        assert_eq!(row.page_id, uuid(11));
        assert_eq!(row.owned_by_id, uuid(7));
        assert_eq!(row.stripped.as_deref(), Some("hi"));
        assert_eq!(row.sub_pages_data, Value::Object(Map::new()));
        assert_eq!(row.last_saved_at, now);
        // Update path: `updated_at` (not `last_saved_at`) + empty sub-pages.
        let upd = build_page_version_update(&snap, now);
        assert_eq!(upd.updated_at, now);
        assert_eq!(upd.sub_pages_data, Value::Object(Map::new()));
        assert_eq!(upd.stripped.as_deref(), Some("hi"));
    }
}
