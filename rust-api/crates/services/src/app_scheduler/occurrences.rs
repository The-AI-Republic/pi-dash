#![forbid(unsafe_code)]

//! Scheduler occurrences query units (D-36, stage 5).
//!
//! Ports the query layer behind
//! `GET /workspaces/<slug>/projects/<project_id>/scheduler-bindings/occurrences/`:
//!
//! * window parsing/validation (`occurrences.py:54-103`) plus the
//!   `parse_iso_utc` half of `utils/iso_datetime.py:19-33` it imports;
//! * future-bindings filter + RRULE-expansion orchestration
//!   (`occurrences.py:105-160`);
//! * past-runs query (`occurrences.py:162-192`);
//! * merge/sort/cap (`occurrences.py:194-199`).
//!
//! Sources (drift baseline `01a93e17`):
//! - `app/views/scheduler/occurrences.py:1-208` — the whole endpoint view
//!   (the permission gate `:71-74` and the project 404 `:78` belong to the
//!   handler issue; the envelope `:201-207` is rendered by the handler
//!   from the pieces here).
//! - `utils/iso_datetime.py:19-33` — `parse_iso_utc`.
//! - `db/models/scheduler.py:107-251` — table/column names (already ported
//!   by D-10; referenced, never re-ported).
//! - `runner/models.py:872-~1040` — the `agent_run` columns this query
//!   reads (the model port belongs to D-15 L2 PIDASHCONV-528, Backlog —
//!   not waited on; columns pinned by F36-06/F36-08).
//!
//! Fixture oracles: F36-07
//! (`queries/occurrences_window.golden.json`: caps, `parse_iso_utc`
//! vectors, independent defaults, both 400 bodies) and F36-08
//! (`queries/occurrences_merge.golden.json`: future filter conjuncts,
//! expansion call shape, past SELECT scope/range/order, string sort, cap
//! and truncation rules). The unit tests below pin every builder against
//! those files so transcription drift fails the build.
//!
//! # Seams
//!
//! This crate holds no database handle, so — like every sibling port —
//! the Django ORM calls become contract SQL ([`future_bindings_sql`],
//! [`past_runs_sql`]) the handler executes via `sqlx::query` (no
//! `query!` macros: there is no build-time database), with the pure
//! decisions living here. Placeholders are PostgreSQL `$N` binds (the
//! D-29 `app_views_search` precedent); each builder documents its binds
//! in order. Row structs ([`FutureBindingRow`], [`PastRunRow`]) carry the
//! selected columns in the handler's decode shape.
//!
//! Per-binding RRULE expansion arrives as an injected `expand_binding`
//! closure ([`collect_future_occurrences`]): inputs are the binding's
//! `dtstart`, `rrule`, `tzid`, the RAW `rdates`/`exdates` JSON values,
//! the future slice bounds and the shrinking remaining cap; output is
//! `(Vec<DateTime<Utc>>, hit_cap)`. The closure takes raw JSON (not
//! coerced datetimes) precisely so ISO coercion stays with the merged
//! RRULE engine the occurrences handler (PIDASHCONV-635) closes over —
//! this module coerces nothing. There is no public shared port of
//! `parse_iso_utc` (only a private copy in the jobs ticker's fire path),
//! so [`parse_iso_utc`] is ported locally here: it is the translation of
//! `occurrences.py`'s import, not a fork of a public helper. This file
//! must never name the jobs crate (jobs already depends on services; the
//! crate graph is acyclic, Porting guide).
//!
//! # Ported quirks (translate, don't redesign — also listed in the PR)
//!
//! 1. The past query carries NO `scheduler_bindings.deleted_at` filter
//!    and NO `projects.deleted_at` filter (`:166-177`; F36-08 `quirks`):
//!    runs whose binding (or scheduler) was since soft-deleted are STILL
//!    included. Only `scheduler_binding__project_id` equality scopes it.
//! 2. Past-driven cap overflow is SILENT (`:178-180`): the past loop
//!    breaks at 5000 rows WITHOUT setting `truncated_at`, so `has_more`
//!    stays false and `next_window_start` stays null.
//! 3. The merge sorts by the `dtstart` STRING (`:196`), not
//!    chronologically — lexicographic order, which equals chronological
//!    order only while every row carries the same `+00:00` offset.
//! 4. `parse_iso_utc` replaces EVERY `Z`, not just a trailing suffix
//!    (`iso_datetime.py:28`); aware offsets convert to UTC, naive values
//!    get UTC attached.
//! 5. `truncated_at` stays `None` when a binding reports `hit_cap` with an
//!    empty expansion (`:158-160`: `expanded[-1] if expanded else None`).
//! 6. A window that fills to exactly 5000 rows WITHOUT any binding
//!    reporting `hit_cap` reports `has_more == false` (same lines).
//!
//! # Deliberate scope
//!
//! [`parse_iso_utc`] covers the fixture-pinned grammar (RFC 3339 aware,
//! space-separator aware, naive datetime with `T`/space, date-only) plus
//! everything the merged first translation of the same Python function
//! accepts. Exotic `fromisoformat`-only shapes (basic `YYYYMMDD`, week
//! dates, comma fractions) fall back to `None` → window defaults — the
//! same failure mode as garbage input, and unpinned by any fixture.
//!
//! Out of scope (owned by the occurrences handler, PIDASHCONV-635): the
//! project existence/scope 404, route registration, the
//! `{occurrences, has_more, next_window_start}` envelope rendering, and
//! the `expand_binding` closure body.

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Caps and defaults (occurrences.py:54-55,86-89)
// ---------------------------------------------------------------------------

/// `MAX_WINDOW_DAYS` (`occurrences.py:54`): the `from`/`to` window must be
/// `<= 90` days, else 400 `window_too_large`.
pub const MAX_WINDOW_DAYS: i64 = 90;

/// `OCCURRENCE_CAP` (`occurrences.py:55`): the merged (past + future)
/// total is capped at 5000 rows.
pub const OCCURRENCE_CAP: usize = 5000;

/// Default window half-width (`occurrences.py:86-89`): missing/garbage
/// bounds default INDEPENDENTLY to `now ± 30 days`.
pub const DEFAULT_WINDOW_DAYS: i64 = 30;

/// `scheduler.color or "#3b82f6"` (`occurrences.py:150,186`): the display
/// fallback when a scheduler row carries an empty color.
pub const DEFAULT_SCHEDULER_COLOR: &str = "#3b82f6";

/// `binding.tzid or "UTC"` (`occurrences.py:138,152,188`): the tzid
/// fallback when a binding row carries an empty tzid.
pub const DEFAULT_TZID: &str = "UTC";

// ---------------------------------------------------------------------------
// Unit 1 — window parsing/validation (occurrences.py:54-103)
// ---------------------------------------------------------------------------

/// Parse an ISO 8601 datetime string into a tz-aware UTC datetime
/// (`utils/iso_datetime.py:19-33`).
///
/// `None`/empty → `None`; `value.replace("Z", "+00:00")` (EVERY `Z`,
/// quirk 4) then `fromisoformat` (`ValueError` → `None`); naive → UTC
/// attached; aware → converted to UTC. The fallback chain mirrors the
/// merged first translation of the same function: strict RFC 3339 (any
/// numeric offset), space-separator with offset (mangled to `T`), naive
/// `T`/space datetimes with optional fraction, date-only at midnight.
/// Anything else → `None` (the caller applies window defaults).
pub fn parse_iso_utc(value: Option<&str>) -> Option<DateTime<Utc>> {
    let value = value?;
    if value.is_empty() {
        return None;
    }
    // Python `str.replace` hits every occurrence, not just a suffix.
    let normalized = value.replace('Z', "+00:00");
    if let Ok(aware) = DateTime::parse_from_rfc3339(&normalized) {
        return Some(aware.with_timezone(&Utc));
    }
    // `datetime.fromisoformat` also accepts a space separator with an offset.
    if normalized.contains(' ') {
        let mangled = normalized.replacen(' ', "T", 1);
        if let Ok(aware) = DateTime::parse_from_rfc3339(&mangled) {
            return Some(aware.with_timezone(&Utc));
        }
    }
    // Naive → assume UTC. `%.f` matches with or without a fraction.
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(&normalized, format) {
            return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
        }
    }
    if let Ok(date) = NaiveDate::parse_from_str(&normalized, "%Y-%m-%d") {
        let naive = date.and_hms_opt(0, 0, 0)?;
        return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
    }
    None
}

/// The two window rejections (`occurrences.py:91-103`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowError {
    /// `to < from` → 400 `{"error": "invalid_window", ...}` (`:91-95`).
    InvalidWindow,
    /// `to - from > 90 days` → 400 `{"error": "window_too_large", ...}`
    /// (`:96-103`).
    WindowTooLarge,
}

impl WindowError {
    /// The `error` key of the 400 body.
    pub const fn error_code(self) -> &'static str {
        match self {
            WindowError::InvalidWindow => "invalid_window",
            WindowError::WindowTooLarge => "window_too_large",
        }
    }

    /// The `detail` message of the 400 body (lowercase — these are
    /// explicit `Response` dicts, not DRF exception bodies — byte-pinned
    /// by F36-07 and the contract suite). The too-large text renders
    /// [`MAX_WINDOW_DAYS`] so the two cannot drift.
    pub fn detail(self) -> String {
        match self {
            WindowError::InvalidWindow => "`to` must be >= `from`".to_owned(),
            WindowError::WindowTooLarge => {
                format!("date window must be <= {MAX_WINDOW_DAYS} days")
            }
        }
    }

    /// Both window rejections answer 400.
    pub const fn status_code(self) -> u16 {
        400
    }

    /// The exact 400 body: `{"error": ..., "detail": ...}` in Python dict
    /// order. The handler renders it; key order is pinned by test.
    pub fn body(self) -> WindowErrorBody {
        WindowErrorBody {
            error: self.error_code(),
            detail: self.detail(),
        }
    }
}

/// The window-rejection 400 body (`occurrences.py:92-95,97-102`), fields
/// in Python dict order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowErrorBody {
    /// `"invalid_window"` / `"window_too_large"`.
    pub error: &'static str,
    /// Lowercase `detail` alongside `error` (explicit `Response` dicts —
    /// contrast DRF-exception bodies, which carry a lone capital-`D`
    /// `Detail` key, and gate denials, which carry a lone `error` key).
    pub detail: String,
}

/// Resolve the `from`/`to` query params into a validated window
/// (`occurrences.py:80-103`).
///
/// Each bound parses via [`parse_iso_utc`] and defaults INDEPENDENTLY on
/// `None` (`:86-89`: garbage `from` + valid `to` keeps the valid `to`).
/// `to < from` (strict — equality is OK) rejects with
/// [`WindowError::InvalidWindow`]; a span `> 90 days` (strict — exactly
/// 90 days is OK) rejects with [`WindowError::WindowTooLarge`].
///
/// The handler captures `now` ONCE (`timezone.now()`, `:80`) and reuses
/// the same instant here and in [`future_start`]/[`past_end`].
pub fn resolve_window(
    from_raw: Option<&str>,
    to_raw: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(DateTime<Utc>, DateTime<Utc>), WindowError> {
    let window_start =
        parse_iso_utc(from_raw).unwrap_or_else(|| now - Duration::days(DEFAULT_WINDOW_DAYS));
    let window_end =
        parse_iso_utc(to_raw).unwrap_or_else(|| now + Duration::days(DEFAULT_WINDOW_DAYS));
    if window_end < window_start {
        return Err(WindowError::InvalidWindow);
    }
    if window_end - window_start > Duration::days(MAX_WINDOW_DAYS) {
        return Err(WindowError::WindowTooLarge);
    }
    Ok((window_start, window_end))
}

// ---------------------------------------------------------------------------
// Occurrence rows (occurrences.py:145-156,178-192)
// ---------------------------------------------------------------------------

/// `kind` of an occurrence row: `"scheduled"` (future RRULE expansion,
/// `:153`) or `"past"` (`AgentRun` rows, `:189`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OccurrenceKind {
    /// Future RRULE-expanded occurrence (`agent_run_id`/`status` null).
    Scheduled,
    /// Past `AgentRun` row (carries `agent_run_id` + `status`).
    Past,
}

/// One merged occurrence row: the 9 keys of `OCCURRENCE_KEYS` (contract
/// `test_occurrences.py`, F36-08 `future_row`/`past_rows`), fields in
/// Python dict-insertion order (`binding_id`, `scheduler_id`,
/// `scheduler_name`, `scheduler_color`, `dtstart`, `tzid`, `kind`,
/// `agent_run_id`, `status`).
///
/// `dtstart` is stored already rendered (see [`format_dtstart`]): the
/// merge sorts on this STRING (`occurrences.sort(key=...)`, `:196`), so
/// the sort key and the wire value are one field. `agent_run_id` and
/// `status` serialize as JSON `null` when `None` (no `skip_serializing`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Occurrence {
    /// `str(binding.id)` (future) / `str(run.scheduler_binding_id)` (past).
    pub binding_id: String,
    /// `str(binding.scheduler_id)` / `str(sched.id)`.
    pub scheduler_id: String,
    /// `binding.scheduler.name` / `sched.name`.
    pub scheduler_name: String,
    /// `scheduler.color or "#3b82f6"` (empty falls back too — Python `or`).
    pub scheduler_color: String,
    /// `occ.isoformat()` / `run.started_at.isoformat()`: `+00:00`
    /// suffix, never `Z` (plain `datetime.isoformat`, not DRF rendering).
    pub dtstart: String,
    /// `binding.tzid or "UTC"`.
    pub tzid: String,
    /// `"scheduled"` / `"past"`.
    pub kind: OccurrenceKind,
    /// Always `None` for future rows; `str(run.id)` for past rows.
    pub agent_run_id: Option<String>,
    /// Always `None` for future rows; `run.status` for past rows.
    pub status: Option<String>,
}

impl Occurrence {
    /// Build a `"scheduled"` row from one expanded instant
    /// (`occurrences.py:145-156`).
    pub fn scheduled(binding: &FutureBindingRow, occ: DateTime<Utc>) -> Self {
        Self {
            binding_id: binding.id.to_string(),
            scheduler_id: binding.scheduler_id.to_string(),
            scheduler_name: binding.scheduler_name.clone(),
            scheduler_color: color_or_default(&binding.scheduler_color),
            dtstart: format_dtstart(&occ),
            tzid: tzid_or_utc(&binding.tzid),
            kind: OccurrenceKind::Scheduled,
            agent_run_id: None,
            status: None,
        }
    }

    /// Build a `"past"` row from one `AgentRun` row
    /// (`occurrences.py:178-192`).
    pub fn past(row: &PastRunRow) -> Self {
        Self {
            binding_id: row.binding_id.to_string(),
            scheduler_id: row.scheduler_id.to_string(),
            scheduler_name: row.scheduler_name.clone(),
            scheduler_color: color_or_default(&row.scheduler_color),
            dtstart: format_dtstart(&row.started_at),
            tzid: tzid_or_utc(&row.binding_tzid),
            kind: OccurrenceKind::Past,
            agent_run_id: Some(row.run_id.to_string()),
            status: Some(row.status.clone()),
        }
    }
}

/// `scheduler.color or "#3b82f6"` (`occurrences.py:150,186`): the column
/// is never NULL, but Python `or` also falls back on `""`.
pub fn color_or_default(color: &str) -> String {
    if color.is_empty() {
        DEFAULT_SCHEDULER_COLOR.to_owned()
    } else {
        color.to_owned()
    }
}

/// `binding.tzid or "UTC"` (`occurrences.py:138,152,188`): same `or`
/// semantics as [`color_or_default`].
pub fn tzid_or_utc(tzid: &str) -> String {
    if tzid.is_empty() {
        DEFAULT_TZID.to_owned()
    } else {
        tzid.to_owned()
    }
}

/// Render a UTC instant exactly like `datetime.isoformat()` on a
/// tz-aware UTC datetime (`occurrences.py:151,187,199`): `+00:00` suffix
/// (NOT `Z` — this path never goes through DRF rendering), microseconds
/// as 6 digits only when nonzero. The 9-digit nanos arm mirrors the api
/// serializer kernel for theoretical sub-microsecond values (Postgres
/// `timestamptz` and the RRULE engine never produce them; Python
/// datetimes cannot hold them).
pub fn format_dtstart(dt: &DateTime<Utc>) -> String {
    let base = dt.format("%Y-%m-%dT%H:%M:%S").to_string();
    let nanos = dt.timestamp_subsec_nanos();
    if nanos == 0 {
        format!("{base}+00:00")
    } else if nanos.is_multiple_of(1000) {
        format!("{base}.{:06}+00:00", nanos / 1000)
    } else {
        format!("{base}.{nanos:09}+00:00")
    }
}

// ---------------------------------------------------------------------------
// Unit 2 — future bindings filter + expansion orchestration (:105-160)
// ---------------------------------------------------------------------------

/// Future-bindings read (`occurrences.py:109-119` + model default
/// ordering): enabled bindings on this project whose parent scheduler is
/// enabled and not soft-deleted, mirroring the scanner's filter.
/// `select_related("scheduler")` becomes the explicit schedulers JOIN.
///
/// Binds: `$1` workspace slug (text), `$2` project id (uuid).
///
/// The projects join is INNER per the F36-08 prose (Django emits
/// `LEFT OUTER JOIN` for the nullable `project` FK, but the
/// `project_id = $2` equality + FK integrity make the two identical on
/// real data: only a dangling-FK row could tell them apart, and the
/// constraint forbids it). Ordering is the model default `-created_at`
/// (`db/models/scheduler.py:235` — the view adds no `order_by`), so the
/// newest binding expands first.
pub fn future_bindings_sql() -> &'static str {
    "SELECT b.id AS id, b.dtstart AS dtstart, b.tzid AS tzid, b.rrule AS rrule, \
     b.rdates AS rdates, b.exdates AS exdates, b.scheduler_id AS scheduler_id, \
     s.name AS scheduler_name, s.color AS scheduler_color \
     FROM scheduler_bindings AS b \
     INNER JOIN schedulers AS s ON b.scheduler_id = s.id \
     INNER JOIN projects AS p ON b.project_id = p.id \
     INNER JOIN workspaces AS w ON b.workspace_id = w.id \
     WHERE b.deleted_at IS NULL AND b.enabled = TRUE AND w.slug = $1 \
     AND b.project_id = $2 AND s.deleted_at IS NULL AND s.is_enabled = TRUE \
     AND p.deleted_at IS NULL \
     ORDER BY b.created_at DESC"
}

/// One future-bindings row: the columns [`future_bindings_sql`] selects.
/// `rrule`/`tzid` store `""`, never NULL; `rdates`/`exdates` are JSON
/// arrays (fresh `[]` per row in Django); `scheduler_color` stores `""`,
/// never NULL (fallbacks apply on empty — Python `or`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FutureBindingRow {
    /// `bindings.id`.
    pub id: Uuid,
    /// `bindings.dtstart` (series anchor, tz-aware UTC).
    pub dtstart: DateTime<Utc>,
    /// `bindings.tzid` (informational today; expansion runs in UTC).
    pub tzid: String,
    /// `bindings.rrule` (empty = single-shot at `dtstart`).
    pub rrule: String,
    /// `bindings.rdates` raw JSON (coerced inside `expand_binding`).
    pub rdates: serde_json::Value,
    /// `bindings.exdates` raw JSON (coerced inside `expand_binding`).
    pub exdates: serde_json::Value,
    /// `bindings.scheduler_id`.
    pub scheduler_id: Uuid,
    /// `schedulers.name` (via `select_related`).
    pub scheduler_name: String,
    /// `schedulers.color` (via `select_related`).
    pub scheduler_color: String,
}

/// `max(window_start, now)` (`occurrences.py:131`): future expansion
/// covers `[future_start, window_end]` only.
pub fn future_start(window_start: DateTime<Utc>, now: DateTime<Utc>) -> DateTime<Utc> {
    window_start.max(now)
}

/// Expand every binding into `"scheduled"` rows (`occurrences.py:130-160`).
///
/// `bindings` must arrive in [`future_bindings_sql`] order (newest
/// first). `expand_binding` is the injected per-binding RRULE engine —
/// `(dtstart, rrule, tzid, raw rdates JSON, raw exdates JSON,
/// window_start, window_end, cap) -> (expanded, hit_cap)` — supplied by
/// the occurrences handler, which closes over the merged engine (it
/// coerces the raw JSON and expands; see PIDASHCONV-635). The cap starts
/// at [`OCCURRENCE_CAP`] and shrinks after each binding
/// (`remaining_cap = 5000 - len(occurrences)`, `:157`); the loop breaks
/// early once it reaches 0 (`:133-134`).
///
/// Returns the future rows plus `truncated_at`: set ONLY when a binding
/// reports `hit_cap` AND the merged total hit exactly 0 remaining
/// (`:158-160`), to the LAST expanded instant of that binding — or
/// `None` when the expansion was empty (quirk 5). `usize` cannot go
/// negative, so `saturating_sub` + `== 0` reproduce Python's `<= 0`
/// checks exactly, including for a contract-violating closure that
/// returns more than `cap` rows (Python would go negative and truncate;
/// saturating clamps to the same outcome without a panic).
pub fn collect_future_occurrences<E>(
    bindings: &[FutureBindingRow],
    start: DateTime<Utc>,
    window_end: DateTime<Utc>,
    mut expand_binding: E,
) -> (Vec<Occurrence>, Option<DateTime<Utc>>)
where
    E: FnMut(
        DateTime<Utc>,
        &str,
        &str,
        &serde_json::Value,
        &serde_json::Value,
        DateTime<Utc>,
        DateTime<Utc>,
        usize,
    ) -> (Vec<DateTime<Utc>>, bool),
{
    let mut occurrences = Vec::new();
    let mut truncated_at = None;
    let mut remaining_cap = OCCURRENCE_CAP;
    for binding in bindings {
        if remaining_cap == 0 {
            break;
        }
        // `rrule or ""` is a no-op on a never-NULL column; `tzid or "UTC"`
        // falls back on empty (quirk-free `or` semantics).
        let (expanded, hit_cap) = expand_binding(
            binding.dtstart,
            &binding.rrule,
            &tzid_or_utc(&binding.tzid),
            &binding.rdates,
            &binding.exdates,
            start,
            window_end,
            remaining_cap,
        );
        for occ in &expanded {
            occurrences.push(Occurrence::scheduled(binding, *occ));
        }
        remaining_cap = OCCURRENCE_CAP.saturating_sub(occurrences.len());
        if hit_cap && remaining_cap == 0 {
            truncated_at = expanded.last().copied();
            break;
        }
    }
    (occurrences, truncated_at)
}

// ---------------------------------------------------------------------------
// Unit 3 — past-runs query (occurrences.py:162-192)
// ---------------------------------------------------------------------------

/// Past-runs read (`occurrences.py:166-177`): `AgentRun` rows over
/// `[window_start, past_end]` scoped by workspace slug +
/// `scheduler_binding__project_id` + non-null binding, ordered by
/// `started_at` ASC (the explicit `:176` order REPLACES the model's
/// `-created_at` default). `select_related("scheduler_binding__scheduler")`
/// becomes the two explicit JOINs.
///
/// Binds: `$1` workspace slug (text), `$2` project id (uuid),
/// `$3` window start (timestamptz), `$4` past end (timestamptz, both
/// bounds INCLUSIVE, `:172-173`).
///
/// Ported quirks (quirk 1): NO `scheduler_bindings.deleted_at` filter
/// and NO projects join at all — runs whose binding was since
/// soft-deleted are STILL included; only the `project_id` equality
/// scopes them. `agent_run` itself has no `deleted_at` column (not a
/// soft-delete model), so no tombstone filter exists on the base table
/// either. The `scheduler_binding_id IS NOT NULL` arm is the explicit
/// translation of `scheduler_binding__isnull=False` (redundant with the
/// INNER JOIN, kept arm-for-arm).
pub fn past_runs_sql() -> &'static str {
    "SELECT r.id AS run_id, r.scheduler_binding_id AS binding_id, \
     r.started_at AS started_at, r.status AS status, b.tzid AS binding_tzid, \
     s.id AS scheduler_id, s.name AS scheduler_name, s.color AS scheduler_color \
     FROM agent_run AS r \
     INNER JOIN scheduler_bindings AS b ON r.scheduler_binding_id = b.id \
     INNER JOIN schedulers AS s ON b.scheduler_id = s.id \
     INNER JOIN workspaces AS w ON r.workspace_id = w.id \
     WHERE w.slug = $1 AND b.project_id = $2 AND r.scheduler_binding_id IS NOT NULL \
     AND r.started_at >= $3 AND r.started_at <= $4 \
     ORDER BY r.started_at ASC"
}

/// One past-runs row: the columns [`past_runs_sql`] selects. `started_at`
/// is non-optional: the range predicates exclude NULLs (Python would
/// `AttributeError` on a NULL `started_at.isoformat()`; a decode failure
/// is the equivalent 500 here).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PastRunRow {
    /// `agent_run.id` → `agent_run_id`.
    pub run_id: Uuid,
    /// `agent_run.scheduler_binding_id` → `binding_id` (non-null by filter).
    pub binding_id: Uuid,
    /// `agent_run.started_at` → `dtstart`.
    pub started_at: DateTime<Utc>,
    /// `agent_run.status` → `status`.
    pub status: String,
    /// `scheduler_bindings.tzid` → `tzid` (empty falls back to UTC).
    pub binding_tzid: String,
    /// `schedulers.id` (via `scheduler_binding.scheduler`).
    pub scheduler_id: Uuid,
    /// `schedulers.name`.
    pub scheduler_name: String,
    /// `schedulers.color` (empty falls back to `#3b82f6`).
    pub scheduler_color: String,
}

/// `min(window_end, now)` (`occurrences.py:165`): the past slice covers
/// `[window_start, past_end]` — no point joining future `AgentRun` rows.
pub fn past_end(window_end: DateTime<Utc>, now: DateTime<Utc>) -> DateTime<Utc> {
    window_end.min(now)
}

/// `window_start < past_end` (`occurrences.py:166`): when false (a
/// fully-future window) the handler skips the past query ENTIRELY.
pub fn past_query_needed(window_start: DateTime<Utc>, past_end: DateTime<Utc>) -> bool {
    window_start < past_end
}

/// Append `"past"` rows (`occurrences.py:178-192`): `rows` must arrive in
/// [`past_runs_sql`] order (`started_at` ASC). The loop breaks once the
/// merged total reaches [`OCCURRENCE_CAP`] WITHOUT setting any truncation
/// signal (quirk 2 — past-driven overflow is silent).
pub fn collect_past_occurrences(rows: &[PastRunRow], occurrences: &mut Vec<Occurrence>) {
    for row in rows {
        if occurrences.len() >= OCCURRENCE_CAP {
            break;
        }
        occurrences.push(Occurrence::past(row));
    }
}

// ---------------------------------------------------------------------------
// Unit 4 — merge/sort/cap (occurrences.py:194-199)
// ---------------------------------------------------------------------------

/// Sort the merged past + future rows by the `dtstart` STRING
/// (`occurrences.sort(key=lambda o: o["dtstart"])`, `:196`). Rust
/// `sort_by` is stable, like Python's `sort`, so equal instants keep
/// their future-before-past interleaving (quirk 3).
pub fn sort_occurrences(occurrences: &mut [Occurrence]) {
    occurrences.sort_by(|a, b| a.dtstart.cmp(&b.dtstart));
}

/// `truncated_at is not None` (`occurrences.py:198`).
pub fn has_more(truncated_at: Option<DateTime<Utc>>) -> bool {
    truncated_at.is_some()
}

/// `truncated_at.isoformat() or None` (`occurrences.py:199`): the
/// `+00:00`-rendered last expanded instant of the binding that hit the
/// cap, else null.
pub fn next_window_start(truncated_at: Option<DateTime<Utc>>) -> Option<String> {
    truncated_at.map(|dt| format_dtstart(&dt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const FIXTURE_WINDOW: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_scheduler/queries/occurrences_window.golden.json"
    );
    const FIXTURE_MERGE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_scheduler/queries/occurrences_merge.golden.json"
    );

    fn fixture(path: &str) -> serde_json::Value {
        let raw = std::fs::read_to_string(path).expect("fixture exists");
        serde_json::from_str(&raw).expect("fixture is valid JSON")
    }

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
    }

    fn binding_row(id_seed: u8) -> FutureBindingRow {
        FutureBindingRow {
            id: Uuid::from_bytes([id_seed; 16]),
            dtstart: utc(2024, 5, 1, 7, 0, 0),
            tzid: "UTC".to_owned(),
            rrule: "FREQ=HOURLY".to_owned(),
            rdates: serde_json::Value::Array(Vec::new()),
            exdates: serde_json::Value::Array(Vec::new()),
            scheduler_id: Uuid::from_bytes([id_seed + 100; 16]),
            scheduler_name: "Nightly".to_owned(),
            scheduler_color: "#10b981".to_owned(),
        }
    }

    fn past_row(started_at: DateTime<Utc>) -> PastRunRow {
        PastRunRow {
            run_id: Uuid::from_bytes([7; 16]),
            binding_id: Uuid::from_bytes([8; 16]),
            started_at,
            status: "completed".to_owned(),
            binding_tzid: "UTC".to_owned(),
            scheduler_id: Uuid::from_bytes([9; 16]),
            scheduler_name: "Nightly".to_owned(),
            scheduler_color: "#10b981".to_owned(),
        }
    }

    // F36-07 `caps`: the consts cannot drift from the golden.
    #[test]
    fn window_caps_match_f36_07() {
        let golden = fixture(FIXTURE_WINDOW);
        assert_eq!(
            golden["caps"]["MAX_WINDOW_DAYS"].as_i64().unwrap(),
            MAX_WINDOW_DAYS
        );
        assert_eq!(
            golden["caps"]["OCCURRENCE_CAP"].as_u64().unwrap(),
            OCCURRENCE_CAP as u64
        );
        assert_eq!(
            golden["defaults"]["from"].as_str().unwrap(),
            "now - 30 days"
        );
        assert_eq!(golden["defaults"]["to"].as_str().unwrap(), "now + 30 days");
    }

    // F36-07 `parse_iso_utc.cases`, transcribed arm for arm
    // (`utils/iso_datetime.py:19-33`).
    #[test]
    fn parse_iso_utc_matches_f36_07_vectors() {
        assert_eq!(parse_iso_utc(None), None);
        assert_eq!(parse_iso_utc(Some("")), None);
        // Garbage → None → defaults.
        assert_eq!(parse_iso_utc(Some("not-a-date")), None);
        assert_eq!(parse_iso_utc(Some("2024-13-01T00:00:00Z")), None);
        // `Z` accepted.
        assert_eq!(
            parse_iso_utc(Some("2024-01-02T03:04:05Z")),
            Some(utc(2024, 1, 2, 3, 4, 5))
        );
        // Aware offsets CONVERTED to UTC.
        assert_eq!(
            parse_iso_utc(Some("2024-06-01T12:00:00+05:00")),
            Some(utc(2024, 6, 1, 7, 0, 0))
        );
        // Naive → UTC attached (wall time kept).
        assert_eq!(
            parse_iso_utc(Some("2024-01-01T00:00:00")),
            Some(utc(2024, 1, 1, 0, 0, 0))
        );
        // Space separator accepted.
        assert_eq!(
            parse_iso_utc(Some("2024-01-01 00:00:00")),
            Some(utc(2024, 1, 1, 0, 0, 0))
        );
        // Date-only → midnight.
        assert_eq!(
            parse_iso_utc(Some("2024-01-01")),
            Some(utc(2024, 1, 1, 0, 0, 0))
        );
        // Fractions survive the round trip.
        let with_micros = parse_iso_utc(Some("2024-01-02T03:04:05.123456Z")).unwrap();
        assert_eq!(with_micros.timestamp_subsec_micros(), 123_456);
        // Quirk 4: EVERY `Z` is replaced — a non-suffix `Z` still parses
        // only when the result is a valid datetime (here it is not).
        assert_eq!(parse_iso_utc(Some("ZZ")), None);
        assert_eq!(parse_iso_utc(Some("2024-01-01T00:00:00+00:00junk")), None);
    }

    // F36-07 `validation.cases`: both 400 bodies byte for byte —
    // golden-Value equality plus serialized key order (`error`, `detail`).
    #[test]
    fn window_error_bodies_match_f36_07() {
        let golden = fixture(FIXTURE_WINDOW);
        let cases = golden["validation"]["cases"].as_array().unwrap();
        let invalid = cases[0]["out_400"].clone();
        let too_large = cases[1]["out_400"].clone();

        assert_eq!(WindowError::InvalidWindow.status_code(), 400);
        assert_eq!(WindowError::WindowTooLarge.status_code(), 400);
        assert_eq!(
            serde_json::to_value(WindowError::InvalidWindow.body()).unwrap(),
            invalid
        );
        assert_eq!(
            serde_json::to_value(WindowError::WindowTooLarge.body()).unwrap(),
            too_large
        );
        assert_eq!(
            serde_json::to_string(&WindowError::InvalidWindow.body()).unwrap(),
            "{\"error\":\"invalid_window\",\"detail\":\"`to` must be >= `from`\"}"
        );
        assert_eq!(
            serde_json::to_string(&WindowError::WindowTooLarge.body()).unwrap(),
            "{\"error\":\"window_too_large\",\"detail\":\"date window must be <= 90 days\"}"
        );
    }

    // F36-07 `defaults.rule`: each bound defaults INDEPENDENTLY —
    // garbage `from` + valid `to` keeps the valid `to`.
    #[test]
    fn resolve_window_defaults_each_bound_independently() {
        let now = utc(2024, 5, 15, 12, 0, 0);
        let (from, to) = resolve_window(None, None, now).unwrap();
        assert_eq!(from, now - Duration::days(30));
        assert_eq!(to, now + Duration::days(30));

        let (from, to) =
            resolve_window(Some("not-a-date"), Some("2024-06-01T00:00:00Z"), now).unwrap();
        assert_eq!(from, now - Duration::days(30));
        assert_eq!(to, utc(2024, 6, 1, 0, 0, 0));

        let (from, to) =
            resolve_window(Some("2024-05-01T00:00:00Z"), Some("also-not-a-date"), now).unwrap();
        assert_eq!(from, utc(2024, 5, 1, 0, 0, 0));
        assert_eq!(to, now + Duration::days(30));
    }

    // F36-07 `validation.cases`: strict `<` / strict `>` only.
    #[test]
    fn resolve_window_validation_boundaries() {
        let now = utc(2024, 5, 15, 12, 0, 0);
        // `to < from` rejects; `to == from` is OK.
        assert_eq!(
            resolve_window(
                Some("2024-05-02T00:00:00Z"),
                Some("2024-05-01T00:00:00Z"),
                now
            ),
            Err(WindowError::InvalidWindow)
        );
        assert!(resolve_window(
            Some("2024-05-01T00:00:00Z"),
            Some("2024-05-01T00:00:00Z"),
            now
        )
        .is_ok());
        // Span > 90 days rejects; exactly 90 days is OK.
        assert_eq!(
            resolve_window(
                Some("2024-01-01T00:00:00Z"),
                Some("2024-04-01T00:00:01Z"),
                now
            ),
            Err(WindowError::WindowTooLarge)
        );
        assert!(resolve_window(
            Some("2024-01-01T00:00:00Z"),
            Some("2024-03-31T00:00:00Z"),
            now
        )
        .is_ok());
    }

    // F36-08 `future_bindings_filter`: every conjunct present, newest
    // first, `$1` slug / `$2` project id.
    #[test]
    fn future_bindings_sql_matches_f36_08() {
        // Table names cannot drift from the D-10 models port.
        assert_eq!(
            pidash_db::tasks_ticker::models::scheduler_binding::TABLE,
            "scheduler_bindings"
        );
        assert_eq!(
            pidash_db::tasks_ticker::models::scheduler::TABLE,
            "schedulers"
        );
        assert_eq!(
            pidash_db::tasks_ticker::models::scheduler_binding::ORDERING,
            "-created_at"
        );

        let sql = future_bindings_sql();
        for fragment in [
            "FROM scheduler_bindings AS b",
            "INNER JOIN schedulers AS s ON b.scheduler_id = s.id",
            "INNER JOIN projects AS p ON b.project_id = p.id",
            "INNER JOIN workspaces AS w ON b.workspace_id = w.id",
            "b.deleted_at IS NULL",
            "b.enabled = TRUE",
            "w.slug = $1",
            "b.project_id = $2",
            "s.deleted_at IS NULL",
            "s.is_enabled = TRUE",
            "p.deleted_at IS NULL",
            "ORDER BY b.created_at DESC",
            // The expansion inputs travel with the row (F36-08
            // `occurrences_between_call_shape.kwargs`).
            "b.dtstart AS dtstart",
            "b.tzid AS tzid",
            "b.rrule AS rrule",
            "b.rdates AS rdates",
            "b.exdates AS exdates",
            "s.name AS scheduler_name",
            "s.color AS scheduler_color",
        ] {
            assert!(sql.contains(fragment), "missing {fragment} in:\n{sql}");
        }
        // The golden records the same shape (prose SQL + conjuncts).
        let golden = fixture(FIXTURE_MERGE);
        let prose = golden["future_bindings_filter"]["sql"].as_str().unwrap();
        for table in ["scheduler_bindings", "projects", "schedulers", "workspaces"] {
            assert!(prose.contains(table), "golden lacks {table}");
        }
    }

    // F36-08 `past_rows`: scope + inclusive range + `started_at` ASC —
    // and the ported-quirk negatives (quirk 1).
    #[test]
    fn past_runs_sql_matches_f36_08() {
        assert_eq!(pidash_db::dispatch::agent_run::TABLE, "agent_run");

        let sql = past_runs_sql();
        for fragment in [
            "FROM agent_run AS r",
            "INNER JOIN scheduler_bindings AS b ON r.scheduler_binding_id = b.id",
            "INNER JOIN schedulers AS s ON b.scheduler_id = s.id",
            "INNER JOIN workspaces AS w ON r.workspace_id = w.id",
            "w.slug = $1",
            "b.project_id = $2",
            "r.scheduler_binding_id IS NOT NULL",
            "r.started_at >= $3",
            "r.started_at <= $4",
            "ORDER BY r.started_at ASC",
        ] {
            assert!(sql.contains(fragment), "missing {fragment} in:\n{sql}");
        }
        // Quirk 1, pinned as negatives: no binding tombstone filter, no
        // projects join at all.
        for absent in ["b.deleted_at", "scheduler_bindings.deleted_at", "projects"] {
            assert!(
                !sql.contains(absent),
                "unexpected guard {absent} in:\n{sql}"
            );
        }
        // The golden records the same scope/range/order.
        let golden = fixture(FIXTURE_MERGE);
        assert_eq!(
            golden["past_rows"]["range"].as_str().unwrap(),
            "started_at >= window_start AND started_at <= past_end (both INCLUSIVE, :172-173)"
        );
        assert_eq!(
            golden["past_rows"]["order"].as_str().unwrap(),
            "agent_run.started_at ASC (:176)"
        );
    }

    // F36-08 `cap.future_rule` + `truncation_rule`: the first binding
    // fills the cap with `hit_cap` → `truncated_at` is its last instant
    // and the second binding is never expanded.
    #[test]
    fn collect_future_shrinks_cap_and_sets_truncated_at() {
        let start = utc(2024, 5, 15, 12, 0, 0);
        let end = utc(2024, 6, 14, 12, 0, 0);
        let calls = std::cell::Cell::new(0);
        let expand = |_dtstart: DateTime<Utc>,
                      _rrule: &str,
                      _tzid: &str,
                      _rdates: &serde_json::Value,
                      _exdates: &serde_json::Value,
                      _window_start: DateTime<Utc>,
                      _window_end: DateTime<Utc>,
                      cap: usize|
         -> (Vec<DateTime<Utc>>, bool) {
            calls.set(calls.get() + 1);
            let rows = (0..cap)
                .map(|i| start + Duration::minutes(i as i64))
                .collect();
            (rows, true)
        };
        let bindings = [binding_row(1), binding_row(2)];
        let (occurrences, truncated_at) = collect_future_occurrences(&bindings, start, end, expand);
        assert_eq!(calls.get(), 1);
        assert_eq!(occurrences.len(), OCCURRENCE_CAP);
        assert!(occurrences
            .iter()
            .all(|o| o.kind == OccurrenceKind::Scheduled));
        assert_eq!(
            truncated_at,
            Some(start + Duration::minutes((OCCURRENCE_CAP - 1) as i64))
        );
        assert!(has_more(truncated_at));
    }

    // F36-08 `cap.truncation_rule`: a window that fills to exactly 5000
    // WITHOUT any `hit_cap` reports no truncation (quirk 6).
    #[test]
    fn collect_future_without_hit_cap_reports_no_truncation() {
        let start = utc(2024, 5, 15, 12, 0, 0);
        let end = utc(2024, 6, 14, 12, 0, 0);
        let expand = |_dtstart: DateTime<Utc>,
                      _rrule: &str,
                      _tzid: &str,
                      _rdates: &serde_json::Value,
                      _exdates: &serde_json::Value,
                      _window_start: DateTime<Utc>,
                      _window_end: DateTime<Utc>,
                      cap: usize|
         -> (Vec<DateTime<Utc>>, bool) {
            let rows = (0..cap)
                .map(|i| start + Duration::minutes(i as i64))
                .collect();
            (rows, false)
        };
        let bindings = [binding_row(1), binding_row(2)];
        let (occurrences, truncated_at) = collect_future_occurrences(&bindings, start, end, expand);
        assert_eq!(occurrences.len(), OCCURRENCE_CAP);
        assert_eq!(truncated_at, None);
        assert!(!has_more(truncated_at));
        assert_eq!(next_window_start(truncated_at), None);
    }

    // F36-08 `has_more_next_window_start`: `hit_cap` with an EMPTY
    // expansion leaves `truncated_at` None (quirk 5).
    #[test]
    fn collect_future_empty_expansion_with_hit_cap_sets_no_truncated_at() {
        let start = utc(2024, 5, 15, 12, 0, 0);
        let end = utc(2024, 6, 14, 12, 0, 0);
        let expand = |_dtstart: DateTime<Utc>,
                      _rrule: &str,
                      _tzid: &str,
                      _rdates: &serde_json::Value,
                      _exdates: &serde_json::Value,
                      _window_start: DateTime<Utc>,
                      _window_end: DateTime<Utc>,
                      _cap: usize|
         -> (Vec<DateTime<Utc>>, bool) { (Vec::new(), true) };
        let bindings = [binding_row(1)];
        let (occurrences, truncated_at) = collect_future_occurrences(&bindings, start, end, expand);
        assert!(occurrences.is_empty());
        assert_eq!(truncated_at, None);
    }

    // The closure receives the F36-08 call shape per binding: dtstart,
    // rrule, tzid (empty → UTC), RAW rdates/exdates JSON, the future
    // slice bounds, and the shrinking cap.
    #[test]
    fn collect_future_passes_call_shape_per_binding() {
        let start = utc(2024, 5, 15, 12, 0, 0);
        let end = utc(2024, 6, 14, 12, 0, 0);
        let mut seen: Vec<(usize, String)> = Vec::new();
        let mut binding = binding_row(1);
        binding.tzid = String::new();
        binding.rdates = serde_json::json!(["2024-05-20T07:00:00Z"]);
        let expand = |dtstart: DateTime<Utc>,
                      rrule: &str,
                      tzid: &str,
                      rdates: &serde_json::Value,
                      exdates: &serde_json::Value,
                      window_start: DateTime<Utc>,
                      window_end: DateTime<Utc>,
                      cap: usize|
         -> (Vec<DateTime<Utc>>, bool) {
            assert_eq!(dtstart, utc(2024, 5, 1, 7, 0, 0));
            assert_eq!(rrule, "FREQ=HOURLY");
            assert_eq!(tzid, "UTC");
            assert_eq!(rdates, &serde_json::json!(["2024-05-20T07:00:00Z"]));
            assert_eq!(exdates, &serde_json::json!([]));
            assert_eq!(window_start, start);
            assert_eq!(window_end, end);
            seen.push((cap, tzid.to_owned()));
            (vec![start], false)
        };
        // Both bindings carry the same raw JSON so the per-call asserts hold.
        let mut second = binding_row(2);
        second.rdates = serde_json::json!(["2024-05-20T07:00:00Z"]);
        let bindings = [binding, second];
        let (occurrences, truncated_at) = collect_future_occurrences(&bindings, start, end, expand);
        // Shrinking cap: 5000 for the first binding, 4999 for the second.
        assert_eq!(
            seen,
            [
                (OCCURRENCE_CAP, "UTC".to_owned()),
                (OCCURRENCE_CAP - 1, "UTC".to_owned())
            ]
        );
        assert_eq!(occurrences.len(), 2);
        assert_eq!(truncated_at, None);
        // Empty binding tzid falls back on the ROW too.
        assert_eq!(occurrences[0].tzid, "UTC");
    }

    // F36-08 `past_rows.guard`: a fully-future window skips the past
    // query entirely (`window_start < past_end`, `:166`).
    #[test]
    fn past_slice_helpers_cover_window_edges() {
        let now = utc(2024, 5, 15, 12, 0, 0);
        let from = utc(2024, 5, 1, 0, 0, 0);
        let to = utc(2024, 6, 1, 0, 0, 0);
        assert_eq!(future_start(from, now), now);
        assert_eq!(
            future_start(now + Duration::days(1), now),
            now + Duration::days(1)
        );
        assert_eq!(past_end(to, now), now);
        assert_eq!(
            past_end(now - Duration::days(1), now),
            now - Duration::days(1)
        );
        assert!(past_query_needed(from, past_end(to, now)));
        // Fully-future window: `from >= past_end` → skip.
        assert!(!past_query_needed(to, past_end(to, now)));
        // Degenerate: `from == past_end` → skip (strict `<`).
        assert!(!past_query_needed(now, now));
    }

    // F36-08 `cap.past_rule`: past fills the rest and breaks at 5000 —
    // silently (quirk 2: no truncation signal exists on this path).
    #[test]
    fn collect_past_fills_silently_to_cap() {
        let mut occurrences: Vec<Occurrence> = (0..OCCURRENCE_CAP - 1)
            .map(|_| Occurrence::scheduled(&binding_row(1), utc(2024, 6, 1, 0, 0, 0)))
            .collect();
        let rows: Vec<PastRunRow> = (0..5)
            .map(|i| past_row(utc(2024, 5, 1, 0, 0, 0) + Duration::hours(i)))
            .collect();
        collect_past_occurrences(&rows, &mut occurrences);
        assert_eq!(occurrences.len(), OCCURRENCE_CAP);
        assert_eq!(occurrences[OCCURRENCE_CAP - 1].kind, OccurrenceKind::Past);
    }

    // F36-08 `sort`: stable STRING sort over the interleaved rows
    // (quirk 3).
    #[test]
    fn sort_occurrences_sorts_by_dtstart_string_stably() {
        let binding = binding_row(1);
        let mut rows = vec![
            Occurrence::scheduled(&binding, utc(2024, 6, 1, 0, 0, 0)),
            Occurrence::past(&past_row(utc(2024, 5, 1, 0, 0, 0))),
            // Same instant as the past row: stability keeps the
            // future-before-past interleaving (future rows are appended
            // first, `:145-156` before `:178-192`).
            Occurrence::scheduled(&binding, utc(2024, 5, 1, 0, 0, 0)),
        ];
        // Interleave future-first, then sort.
        rows.swap(1, 2);
        sort_occurrences(&mut rows);
        let dts: Vec<&str> = rows.iter().map(|o| o.dtstart.as_str()).collect();
        assert_eq!(
            dts,
            [
                "2024-05-01T00:00:00+00:00",
                "2024-05-01T00:00:00+00:00",
                "2024-06-01T00:00:00+00:00",
            ]
        );
        assert_eq!(rows[0].kind, OccurrenceKind::Scheduled);
        assert_eq!(rows[1].kind, OccurrenceKind::Past);
    }

    // F36-08 `future_row`/`past_rows` + contract `OCCURRENCE_KEYS`: 9 keys
    // in Python dict order, fallbacks, nulls.
    #[test]
    fn occurrence_rows_carry_nine_keys_and_fallbacks() {
        let mut binding = binding_row(1);
        binding.scheduler_color = String::new();
        binding.tzid = String::new();
        let scheduled = Occurrence::scheduled(&binding, utc(2024, 5, 7, 7, 0, 0));
        let scheduled_value = serde_json::to_value(&scheduled).unwrap();
        let scheduled_obj = scheduled_value.as_object().unwrap();
        let scheduled_keys: Vec<&str> = scheduled_obj.keys().map(String::as_str).collect();
        assert_eq!(
            scheduled_keys,
            [
                "binding_id",
                "scheduler_id",
                "scheduler_name",
                "scheduler_color",
                "dtstart",
                "tzid",
                "kind",
                "agent_run_id",
                "status",
            ]
        );
        assert_eq!(scheduled.scheduler_color, "#3b82f6");
        assert_eq!(scheduled.tzid, "UTC");
        assert_eq!(scheduled.kind, OccurrenceKind::Scheduled);
        assert_eq!(scheduled_value["agent_run_id"], serde_json::Value::Null);
        assert_eq!(scheduled_value["status"], serde_json::Value::Null);
        assert_eq!(scheduled_value["kind"].as_str().unwrap(), "scheduled");
        // F36-08 `future_row.example.dtstart`.
        assert_eq!(scheduled.dtstart, "2024-05-07T07:00:00+00:00");

        let mut run = past_row(utc(2024, 5, 4, 6, 0, 0));
        run.scheduler_color = String::new();
        run.binding_tzid = String::new();
        let past = Occurrence::past(&run);
        let past_value = serde_json::to_value(&past).unwrap();
        let past_keys: Vec<&str> = past_value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(past_keys, scheduled_keys);
        assert_eq!(past.kind, OccurrenceKind::Past);
        assert_eq!(past.scheduler_color, "#3b82f6");
        assert_eq!(past.tzid, "UTC");
        assert_eq!(past.agent_run_id, Some(run.run_id.to_string()));
        assert_eq!(past.status, Some("completed".to_owned()));
        assert_eq!(past.dtstart, "2024-05-04T06:00:00+00:00");
    }

    // `datetime.isoformat()` on aware-UTC datetimes: `+00:00`, 6-digit
    // micros only when nonzero.
    #[test]
    fn format_dtstart_matches_python_isoformat() {
        assert_eq!(
            format_dtstart(&utc(2024, 5, 7, 7, 0, 0)),
            "2024-05-07T07:00:00+00:00"
        );
        let micros = utc(2024, 5, 7, 7, 0, 0) + Duration::microseconds(123_456);
        assert_eq!(format_dtstart(&micros), "2024-05-07T07:00:00.123456+00:00");
        // Trailing-zero micros keep all 6 digits, like Python.
        let trailing = utc(2024, 5, 7, 7, 0, 0) + Duration::microseconds(123_000);
        assert_eq!(
            format_dtstart(&trailing),
            "2024-05-07T07:00:00.123000+00:00"
        );
    }

    // F36-08 `has_more_next_window_start`: both derive ONLY from the
    // future-truncation path.
    #[test]
    fn truncation_signals_derive_from_truncated_at_only() {
        let at = utc(2024, 6, 14, 11, 59, 0);
        assert!(has_more(Some(at)));
        assert_eq!(
            next_window_start(Some(at)),
            Some("2024-06-14T11:59:00+00:00".to_owned())
        );
        assert!(!has_more(None));
        assert_eq!(next_window_start(None), None);
    }
}
