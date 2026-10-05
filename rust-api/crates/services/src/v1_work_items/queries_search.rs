#![forbid(unsafe_code)]

//! v1 work-item search + page read queries (D-18 queries C, PIDASHCONV-670).
//!
//! Ports the read querysets behind the v1 search endpoints and the page read
//! endpoints to SQL text plus param/assembly helpers, following the D-25
//! precedent (`app_project/queries.rs`, also followed by sibling Q1
//! `queries_core.rs` on PR #1090): builders return fragments the handler
//! splices into the statement it executes, with symbolic `:name`
//! placeholders the handler binds. Fragments compose into full statements
//! via the `*_sql` composers below.
//!
//! Sources (drift baseline `01a93e17`; all paths under `apps/api/pi_dash/`):
//! - `api/views/issue.py:2654-2722` — `IssueSearchEndpoint.get` (legacy).
//! - `api/views/issue.py:2723-2917` — `IssueAdvancedSearchEndpoint.get`.
//! - `search/issue.py:128-186` — `issue_search_queryset` (endpoint assembly;
//!   the FTS vectors live in [`fts`](crate::app_views_search::fts), reused).
//! - `search/issue.py:189-198` — `extract_snippet` (reused from `fts`, not
//!   re-ported; named here for app-search/assistant reuse — see below).
//! - `api/views/page.py:173-197` — `BasePageReadAPIEndpoint.get_queryset`.
//! - `api/views/page.py:199-220` — `validate_parent`.
//! - `api/views/page.py:222-226` — `get_page_or_error`.
//! - `api/views/page.py:228-230` — `detail_response`.
//! - `api/views/page.py:480-553` — archive read path (`PageArchiveAPIEndpoint`
//!   fetch + detail select + admin `EXISTS`; the guard *decision* is
//!   PIDASHCONV-671's, the writes are PIDASHCONV-679's).
//! - `db/models/issue.py:95-103` — `IssueManager` scope.
//! - `db/mixins.py:56-68` — `SoftDeletionManager` scope.
//! - `db/models/page.py:23-64` — `Page` access levels, `Meta` ordering.
//! - `db/models/page.py:135-155` — `ProjectPage` through table.
//! - `db/models/project.py:28-30,192-219` — `ROLE` + `Project.resolve`.
//! - `utils/constants.py:76-88` — `STATE_GROUP_ORDER`, open/closed groups.
//!
//! Fixture oracle: F18-08
//! (`rust-api/fixtures/v1_work_items/queries/F18-08.search_page.json`).
//! Fixture SQL carries live values (unquoted `str(query)` rendering) and
//! Django's `UPPER(..) LIKE` spelling; builders emit symbolic placeholders
//! and the crate's single `ILIKE` spelling (see `fts`), so replay tests
//! compare normalized skeletons arm-by-arm instead of raw strings.
//!
//! Reuse (never fork) — named for app-search/assistant reuse, which import
//! the same `search/issue.py` module:
//! - `crate::app_views_search::fts` — the whole FTS closure
//!   (`issue_search_queryset` vectors, `_build_search_filter` arms, `_rank`,
//!   `_headline`, `extract_snippet`, `sequence_tokens`, `escape_like`).
//!   This module ports only the v1 endpoint-specific assembly around it.
//! - `crate::app_issues::ordering::STATE_ORDER` — the same
//!   `STATE_GROUP_ORDER` tuple (`OPEN_STATE_GROUPS` is `[:5]`,
//!   `CLOSED_STATE_GROUPS` is `[5:]`).
//! - `crate::app_issues::params::ParamError` — view-inline `{"error": …}`
//!   400/404 bodies (also the `int()` leniency precedent for limits).
//!
//! Ported bugs and quirks (translation, don't redesign; also listed in the
//! PR):
//! 1. Legacy `?limit=` is an unguarded `int()` (`:2712`): garbage 500s
//!    (`ValueError`), negatives 500 (Django negative-slice
//!    `AssertionError`), and unbounded-huge values 500 (Postgres int8
//!    overflow) — the port renders `LIMIT` as a full-precision literal.
//! 2. Legacy `?project_id=` with a non-UUID 500s (`UUIDField`
//!    `ValidationError`); there is no identifier fallback on this path.
//! 3. `?status=` is unvalidated: anything but `closed`/`open` behaves as
//!    `all`, silently.
//! 4. A UUID-form `?project=` that matches nothing yields an empty 200
//!    (no 404); only the identifier form 404s (via `Project.resolve`).
//! 5. `?since=` well-formed-but-invalid (month 13, Feb 30, hour 24,
//!    offset ≥ 24h) 500s (`parse_datetime`'s `ValueError` propagates);
//!    only malformed input gets the 400 body.
//! 6. `?since=` grammar is `datetime.fromisoformat` (3.12) ∪ `datetime_re`:
//!    date-only means midnight, any single char separates date/time (so
//!    `2025-01-01+05:30` is 05:30, not a timezone), fractions are always
//!    fractional *seconds*, and a trailing fraction after a *nonzero*
//!    offset extends the offset while after a zero offset it is dropped.
//! 7. The comment-text arm ignores `IssueComment.access`, so INTERNAL
//!    comment text can surface an issue to a member who cannot read that
//!    comment (`search/issue.py:78-83`, as-is).
//! 8. Comment-only matches rank 0 (`_rank` is over the issue vector only;
//!    the caller secondary-sorts by recency).
//! 9. Legacy `?search=` is not stripped (whitespace-only searches) while
//!    advanced `?q=` is stripped (whitespace-only returns empty).
//! 10. `workspace_search` matches the exact string `"false"`: `?workspace_search=`
//!     (empty) disables the project filter.
//! 11. `validate_parent` walks ancestors through unscoped `Page.objects`
//!     (cross-project pages are visible to the cycle check); a missing
//!     intermediate page ends the walk silently.
//! 12. Django renders the advanced rank ordering positionally
//!     (`ORDER BY 10 DESC`); the port orders by the `"_rank"` alias —
//!     same rows, stable under select-list width.
//! 13. F18-08's recorded advanced SQL omits the `updated_at`/`completed_at`
//!     selects (shell-reconstruction artifact — the recorded *rows* carry
//!     all 14 keys); the port follows the source (`values()` order).
//! 14. `int()` limit parsing matches the merged `parse_per_page` leniency
//!     (all underscores stripped, ASCII digits only) rather than byte-exact
//!     CPython (`_1`, `١٢`) — one `int()` behavior across the crate.
//!
//! Out of scope (sibling issues): label/subresource reads (669/Q2),
//! list/detail reads (668/Q1), guards (671/P1), task call sites (672/T1),
//! handler assembly + routes + datetime/URL rendering (677/H-E, 679/H-G),
//! column lists (667), serializer shapes (660-666).

use std::collections::HashSet;
use std::sync::LazyLock;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use regex::Regex;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::app_issues::ordering::STATE_ORDER;
use crate::app_issues::params::ParamError;
use crate::app_views_search::fts;

// ---------------------------------------------------------------------------
// Failure modes
// ---------------------------------------------------------------------------

/// Places where the Python raises and DRF renders a 500. The handler maps
/// every variant to 500 without a body contract (there is none — Django's
/// technical-500 page is not part of the API surface).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SearchBug500 {
    /// Legacy `int(limit)` on a non-integer (`views/issue.py:2718`).
    #[error("legacy limit is not an integer (int(limit) ValueError)")]
    BadLegacyLimit,
    /// Legacy `[:n]` with `n < 0` (Django negative-slice AssertionError).
    #[error("legacy limit is negative (negative slice AssertionError)")]
    NegativeLegacyLimit,
    /// Legacy `filter(project_id=…)` with a non-UUID (`UUIDField`
    /// `ValidationError`).
    #[error("legacy project_id is not a UUID (UUIDField ValidationError)")]
    BadLegacyProjectId,
    /// `?since=` matched `datetime_re` but is not a valid datetime
    /// (`parse_datetime`'s `ValueError` propagates past the view).
    #[error("since is well-formed but not a valid datetime (parse_datetime ValueError)")]
    BadSinceValue,
}

// ---------------------------------------------------------------------------
// Shared search base: IssueManager + view scope + FTS assembly
// ---------------------------------------------------------------------------

/// Join shape under both search endpoints, in fixture order: the
/// `IssueManager` state join (nullable FK → `LEFT OUTER JOIN`), the project
/// traversal, the membership traversal (`project__project_projectmember__…`,
/// no soft-delete arm — traversals never pick up the member manager's
/// scope), and the workspace traversal.
pub const SEARCH_JOINS_SQL: &str = "LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") INNER JOIN \"project_members\" ON (\"projects\".\"id\" = \"project_members\".\"project_id\") INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\")";

/// `IssueManager.get_queryset` (`db/models/issue.py:95-103`), in fixture
/// predicate order. Sibling Q1 (`queries_core.rs`, PR #1090) owns the
/// identical text as `ISSUE_MANAGER_WHERE` for list/detail; it is repeated
/// here (not imported) because Q1 is unmerged and this module must build on
/// `rust-dev` alone.
pub const SEARCH_MANAGER_WHERE: &str = "\"issues\".\"deleted_at\" IS NULL AND NOT (\"states\".\"group\" = 'triage' AND \"states\".\"group\" IS NOT NULL) AND NOT (\"issues\".\"archived_at\" IS NOT NULL) AND NOT (\"projects\".\"archived_at\" IS NOT NULL) AND NOT (\"issues\".\"is_draft\")";

/// The view-level scope both search endpoints apply
/// (`views/issue.py:2696-2701,2809-2814`), in fixture predicate order. Note
/// the `projects.archived_at IS NULL` arm duplicates the manager's
/// `NOT (… IS NOT NULL)` textually — Django renders both.
pub fn search_scope_where() -> String {
    "\"projects\".\"archived_at\" IS NULL AND \"project_members\".\"is_active\" AND \"project_members\".\"member_id\" = :member_id AND \"workspaces\".\"slug\" = :slug".to_owned()
}

/// Manager scope + view scope: the `WHERE` before `issue_search_queryset`.
pub fn search_base_where() -> String {
    format!("{} AND {}", SEARCH_MANAGER_WHERE, search_scope_where())
}

/// The `_build_search_filter` OR-chain for one endpoint
/// (`search/issue.py:93-125`), via [`fts::search_filter_sql`]. Returns
/// `None` for an empty query, mirroring `issue_search_queryset`'s
/// `if not query: return queryset` (`:161-162`) — no FTS arms, no
/// annotations. Symbolic placeholders: `:fts` binds the raw query text,
/// `:seq` binds [`fts::sequence_tokens`]`(query)` as an int array, `:like`
/// binds [`fts::escape_like`]`(query)`.
pub fn search_fts_where(query: &str, include_comments: bool) -> Option<String> {
    if !fts::search_applies(query) {
        return None;
    }
    Some(fts::search_filter_sql(
        include_comments,
        ":fts",
        ":seq",
        ":like",
    ))
}

/// Full search `WHERE`: base scope plus the optional FTS chain.
pub fn search_where(query: &str, include_comments: bool) -> String {
    let base = search_base_where();
    match search_fts_where(query, include_comments) {
        Some(chain) => format!("{base} AND {chain}"),
        None => base,
    }
}

/// The three text-derived binds. The full per-statement bind contract is:
/// `:member_id` (request user UUID), `:slug` (workspace slug),
/// `:project_id` (when a project filter applies), `:since` (timestamptz,
/// advanced only), `:limit` (advanced only, `1..=50`), plus these three.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchTextBinds {
    /// `:fts` — the raw query text (websearch operators flow through).
    pub fts: String,
    /// `:seq` — `search_sequence_tokens(query)` as an int array.
    pub seq: Vec<i64>,
    /// `:like` — `escape_icontains(query)` for the `icontains` arms.
    pub like: String,
}

/// Derive the text binds for `query` (see [`search_fts_where`]).
pub fn search_text_binds(query: &str) -> SearchTextBinds {
    SearchTextBinds {
        fts: query.to_owned(),
        seq: fts::sequence_tokens(query),
        like: fts::escape_like(query),
    }
}

// ---------------------------------------------------------------------------
// Legacy search: IssueSearchEndpoint (views/issue.py:2654-2722)
// ---------------------------------------------------------------------------

/// `.values()` keys in call order (`:2711-2718`).
pub const LEGACY_VALUES_KEYS: &[&str] = &[
    "name",
    "id",
    "sequence_id",
    "project__identifier",
    "project_id",
    "workspace__slug",
];

/// Legacy select list: the six `values()` keys plus the trailing
/// `"issues"."created_at"` Django appends for `DISTINCT` + `ORDER BY`
/// compliance (fixture-pinned).
pub fn legacy_select_sql() -> String {
    "\"issues\".\"name\", \"issues\".\"id\", \"issues\".\"sequence_id\", \"projects\".\"identifier\", \"issues\".\"project_id\", \"workspaces\".\"slug\", \"issues\".\"created_at\"".to_owned()
}

/// Outcome of the legacy project gate
/// (`:2708-2709`: `if workspace_search == "false" and project_id:`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyProject {
    /// No project filter (workspace-wide, or no `project_id` given).
    None,
    /// `AND "issues"."project_id" = :project_id`, binding the parsed
    /// value (Django's `UUIDField` binds the canonical form even for
    /// braced/URN input, which Postgres would reject as raw text).
    Filter(Uuid),
    /// `project_id` is present but not a UUID — Python raises
    /// (`UUIDField` `ValidationError`); the handler 500s.
    InvalidUuid,
}

/// Evaluate the legacy project gate. `workspace_search` defaults to
/// `"false"` when absent and is compared exactly (case-sensitive, no
/// strip — quirk 10); `project_id` must additionally be non-empty.
pub fn legacy_project_filter(
    workspace_search: Option<&str>,
    project_id: Option<&str>,
) -> LegacyProject {
    let workspace_search = workspace_search.unwrap_or("false");
    let Some(pid) = project_id else {
        return LegacyProject::None;
    };
    if workspace_search != "false" || pid.is_empty() {
        return LegacyProject::None;
    }
    match Uuid::parse_str(pid) {
        Ok(id) => LegacyProject::Filter(id),
        Err(_) => LegacyProject::InvalidUuid,
    }
}

/// The legacy/advanced UUID project arm (shared text).
pub const PROJECT_UUID_WHERE: &str = "\"issues\".\"project_id\" = :project_id";

/// Whether the legacy endpoint searches at all (`:2693-2694`):
/// `request.query_params.get("search", False)` is falsy when missing or
/// empty — notably *not* stripped (quirk 9).
pub fn legacy_query_present(query: Option<&str>) -> bool {
    matches!(query, Some(text) if !text.is_empty())
}

/// Legacy `[: int(limit)]` (`:2718`) as a `LIMIT` literal. The default is
/// the int `10`; parsing mirrors the merged `parse_per_page` leniency
/// (trim, strip all underscores, ASCII digits, optional sign — quirk 14).
/// The literal keeps full precision so unbounded-huge limits reach
/// Postgres and 500 on int8 overflow exactly like Django (quirk 1).
pub fn legacy_limit_sql(raw: Option<&str>) -> Result<String, SearchBug500> {
    let text = raw.unwrap_or("10");
    let digits = text.trim().replace('_', "");
    let (negative, body) = match digits.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, digits.strip_prefix('+').unwrap_or(&digits)),
    };
    if body.is_empty() || !body.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(SearchBug500::BadLegacyLimit);
    }
    // Canonicalize like `int()`: `-0` is `0` (`[:0]` → `LIMIT 0`, not a
    // negative slice), leading zeros collapse.
    let canon = body.trim_start_matches('0');
    let canon = if canon.is_empty() { "0" } else { canon };
    if negative && canon != "0" {
        return Err(SearchBug500::NegativeLegacyLimit);
    }
    Ok(format!("LIMIT {canon}"))
}

/// Empty-query envelope (`:2694`), compact DRF separators.
pub const EMPTY_SEARCH_BODY: &str = "{\"issues\":[]}";

/// Full legacy statement. `limit_sql` comes from [`legacy_limit_sql`];
/// ordering is the model `Meta` default (`-created_at` — no explicit
/// `order_by` call). `LegacyProject::InvalidUuid` must never reach here
/// (the handler 500s first); it is treated as no filter. An empty query
/// yields the base statement without FTS arms, mirroring
/// `issue_search_queryset` — the handler early-returns
/// [`EMPTY_SEARCH_BODY`] before executing it.
pub fn legacy_search_sql(query: &str, project: LegacyProject, limit_sql: &str) -> String {
    let mut chain = search_where(query, false);
    if matches!(project, LegacyProject::Filter(_)) {
        chain.push_str(" AND ");
        chain.push_str(PROJECT_UUID_WHERE);
    }
    format!(
        "SELECT DISTINCT {} FROM \"issues\" {} WHERE ({}) ORDER BY \"issues\".\"created_at\" DESC {}",
        legacy_select_sql(),
        SEARCH_JOINS_SQL,
        chain,
        limit_sql,
    )
}

// ---------------------------------------------------------------------------
// Advanced search: IssueAdvancedSearchEndpoint (views/issue.py:2723-2917)
// ---------------------------------------------------------------------------

/// `.values()` keys in call order (`:2860-2875`), annotations in place.
pub const ADVANCED_VALUES_KEYS: &[&str] = &[
    "id",
    "sequence_id",
    "name",
    "_headline",
    "_rank",
    "created_at",
    "updated_at",
    "completed_at",
    "state__name",
    "state__group",
    "project_id",
    "project__identifier",
    "project__name",
    "workspace__slug",
];

/// Advanced select list: the fourteen `values()` keys in call order, with
/// `_rank`/`_headline` rendered by [`fts`] (the headline column is
/// unqualified there; unambiguous — the only `description_stripped` in
/// scope is the outer `issues` row — while Django qualifies it).
pub fn advanced_select_sql() -> String {
    format!(
        "\"issues\".\"id\", \"issues\".\"sequence_id\", \"issues\".\"name\", {} AS \"_headline\", {} AS \"_rank\", \"issues\".\"created_at\", \"issues\".\"updated_at\", \"issues\".\"completed_at\", \"states\".\"name\", \"states\".\"group\", \"issues\".\"project_id\", \"projects\".\"identifier\", \"projects\".\"name\", \"workspaces\".\"slug\"",
        fts::headline_sql(":fts"),
        fts::rank_sql("issues", ":fts"),
    )
}

/// `query = (request.query_params.get("q") or "").strip()` (`:2780`):
/// stripped, unlike legacy (quirk 9). `None` means the endpoint returns
/// [`EMPTY_ADVANCED_BODY`].
pub fn parse_advanced_query(raw: Option<&str>) -> Option<String> {
    let query = raw.unwrap_or("").trim().to_owned();
    if query.is_empty() {
        None
    } else {
        Some(query)
    }
}

/// Empty-`q` envelope (`:2781-2783`), compact DRF separators, wire key order.
pub const EMPTY_ADVANCED_BODY: &str = "{\"query\":\"\",\"count\":0,\"results\":[]}";

/// `_SORT_OPTIONS` keys in source order (`:2750-2754`) — also the order of
/// the `Valid: …` list in the 400 body.
pub const SORT_OPTIONS: &[&str] = &["rank", "-created", "-updated"];

/// Parsed `?sort=` (`(get("sort") or "rank").lower()`, `:2790`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    /// `("-_rank", "-created_at")`.
    Rank,
    /// `("-created_at",)`.
    Created,
    /// `("-updated_at",)`.
    Updated,
}

/// Parse `?sort=` (`(get("sort") or "rank").lower()`, `:2790`): absent
/// *or empty* means `rank`; unknown values 400 with the exact
/// view-inline body. The echoed value is the lowercased input.
pub fn parse_sort(raw: Option<&str>) -> Result<Sort, ParamError> {
    let sort = raw
        .filter(|text| !text.is_empty())
        .unwrap_or("rank")
        .to_lowercase();
    match sort.as_str() {
        "rank" => Ok(Sort::Rank),
        "-created" => Ok(Sort::Created),
        "-updated" => Ok(Sort::Updated),
        _ => Err(ParamError::error(format!(
            "Unknown sort '{sort}'. Valid: {}.",
            SORT_OPTIONS.join(", ")
        ))),
    }
}

/// `ORDER BY` per sort (`:2857`). The rank branch orders by the `"_rank"`
/// alias where Django emits the positional `10` (quirk 12) — same rows.
pub fn sort_order_sql(sort: Sort) -> &'static str {
    match sort {
        Sort::Rank => "\"_rank\" DESC, \"issues\".\"created_at\" DESC",
        Sort::Created => "\"issues\".\"created_at\" DESC",
        Sort::Updated => "\"issues\".\"updated_at\" DESC",
    }
}

/// `_DEFAULT_LIMIT` / `_MAX_LIMIT` (`:2759-2760`).
pub const ADVANCED_DEFAULT_LIMIT: i64 = 10;
/// See [`ADVANCED_DEFAULT_LIMIT`].
pub const ADVANCED_MAX_LIMIT: i64 = 50;

/// `int(get("limit", 10))` with `except (TypeError, ValueError)` → default,
/// clamped to `max(1, min(50, limit))` (`:2803-2807`). Integer parsing is
/// the `parse_per_page` precedent (trim, strip underscores, ASCII digits,
/// sign; quirk 14); magnitudes past `i128` decide by sign alone (a
/// positive one clamps to 50, a negative one to 1 — same as unbounded
/// Python ints through the clamp).
pub fn advanced_limit(raw: Option<&str>) -> i64 {
    let Some(text) = raw else {
        return ADVANCED_DEFAULT_LIMIT;
    };
    let digits = text.trim().replace('_', "");
    let value: i128 = match digits.parse() {
        Ok(value) => value,
        Err(_) => {
            let body: &str = digits.strip_prefix(['+', '-']).unwrap_or(&digits);
            if !body.is_empty() && body.bytes().all(|byte| byte.is_ascii_digit()) {
                // Past `i128`: the sign alone decides the clamped result.
                if digits.starts_with('-') {
                    return 1;
                }
                return ADVANCED_MAX_LIMIT;
            }
            return ADVANCED_DEFAULT_LIMIT;
        }
    };
    value.clamp(1, ADVANCED_MAX_LIMIT as i128) as i64
}

/// `OPEN_STATE_GROUPS` (`utils/constants.py:86`): `STATE_ORDER[:-2]`.
pub fn open_state_groups() -> &'static [&'static str] {
    &STATE_ORDER[..5]
}

/// `CLOSED_STATE_GROUPS` (`utils/constants.py:88`): `STATE_ORDER[-2:]`.
pub fn closed_state_groups() -> &'static [&'static str] {
    &STATE_ORDER[5..]
}

/// Parsed `?status=` (`(get("status") or "all").lower()`, `:2788`).
/// Anything but `closed`/`open` — including garbage — behaves as `all`
/// (quirk 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusFilter {
    /// No status predicate.
    #[default]
    All,
    /// `Q(state__group__in=OPEN) | Q(state__isnull=True)` — the positive
    /// list keeps stateless issues visible (`NULL NOT IN (…)` would drop
    /// them; `:2745-2748`).
    Open,
    /// `state__group__in=CLOSED`.
    Closed,
}

/// Parse `?status=`.
pub fn parse_status(raw: Option<&str>) -> StatusFilter {
    match raw.unwrap_or("all").to_lowercase().as_str() {
        "closed" => StatusFilter::Closed,
        "open" => StatusFilter::Open,
        _ => StatusFilter::All,
    }
}

/// `IN` list over state groups, single-quoted like Django.
fn group_in_list(groups: &[&str]) -> String {
    let items: Vec<String> = groups.iter().map(|group| format!("'{group}'")).collect();
    format!("\"states\".\"group\" IN ({})", items.join(", "))
}

/// Status predicate (`:2829-2834`), or `None` for `all`.
pub fn status_where_sql(status: StatusFilter) -> Option<String> {
    match status {
        StatusFilter::All => None,
        StatusFilter::Closed => Some(group_in_list(closed_state_groups())),
        StatusFilter::Open => Some(format!(
            "({} OR \"issues\".\"state_id\" IS NULL)",
            group_in_list(open_state_groups())
        )),
    }
}

/// Parsed `?project=` (`:2816-2827`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectRef<'a> {
    /// Absent or empty — no project filter.
    None,
    /// Valid UUID — direct `project_id` filter (a miss yields empty rows,
    /// never 404; quirk 4). Carries the parsed value: the handler binds
    /// its canonical form, as Django's `UUIDField` does even for
    /// braced/URN input.
    Uuid(Uuid),
    /// Anything else — `Project.resolve` identifier lookup (404s via
    /// `Http404` when unknown; the handler owns that body).
    Identifier(&'a str),
}

/// Classify `?project=`. UUID validation is [`Uuid::parse_str`], whose
/// accepted forms (hyphenated, simple, braced, URN; no trim) are exactly
/// `uuid.UUID`'s — probed, not assumed.
pub fn parse_project_param(raw: Option<&str>) -> ProjectRef<'_> {
    let Some(text) = raw else {
        return ProjectRef::None;
    };
    if text.is_empty() {
        return ProjectRef::None;
    }
    match Uuid::parse_str(text) {
        Ok(id) => ProjectRef::Uuid(id),
        Err(_) => ProjectRef::Identifier(text),
    }
}

/// Python `str.strip()` membership (`db/models/project.py:210`): Rust
/// `White_Space` plus U+001C-U+001F (verified by exhaustively diffing
/// `str.strip` against `char::is_whitespace` over all code points —
/// those four are the only differences).
fn is_py_strip_ws(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

/// `str(value).strip().upper()` (`db/models/project.py:210`): the
/// identifier normalization before the btree equality lookup.
/// `trim_matches(is_py_strip_ws)` matches `strip` exactly (including
/// U+001C-U+001F); `to_uppercase` matches `upper`.
pub fn normalize_project_identifier(value: &str) -> String {
    value.trim_matches(is_py_strip_ws).to_uppercase()
}

/// Join for the identifier branch of `Project.resolve`
/// (`db/models/project.py:208-212`): the workspace traversal.
pub fn resolve_identifier_joins_sql() -> &'static str {
    "INNER JOIN \"workspaces\" ON (\"projects\".\"workspace_id\" = \"workspaces\".\"id\")"
}

/// `WHERE` for the identifier branch: manager scope + workspace slug +
/// upper-cased identifier equality + the explicit `deleted_at` arm again
/// (Django renders identical stacked predicates twice — the same
/// no-dedupe behavior Q1 ports as its quirk 2).
pub fn resolve_identifier_where_sql() -> &'static str {
    "\"projects\".\"deleted_at\" IS NULL AND \"workspaces\".\"slug\" = :slug AND \"projects\".\"identifier\" = :identifier AND \"projects\".\"deleted_at\" IS NULL"
}

/// `.first()` tail of `Project.resolve`: `Meta` ordering + `LIMIT 1`.
pub fn resolve_identifier_tail_sql() -> &'static str {
    "ORDER BY \"projects\".\"created_at\" DESC LIMIT 1"
}

// ---------------------------------------------------------------------------
// Advanced search: ?since= (parse_datetime port)
// ---------------------------------------------------------------------------

/// The exact 400 message (`:2842-2843`; em-dash U+2014, fixture-pinned).
pub const SINCE_ERROR_MESSAGE: &str =
    "Invalid 'since' — expected ISO 8601 datetime (e.g. 2025-01-01T00:00:00Z).";

/// Outcome of `?since=` parsing (`:2836-2848`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SinceOutcome {
    /// Absent or empty — no `updated_at` filter (`if since_param:`).
    NoFilter,
    /// Parsed; the handler binds it as `:since` (timestamptz, UTC).
    At(DateTime<Utc>),
    /// `parse_datetime` returned `None` — the 400 body.
    BadFormat(ParamError),
    /// `parse_datetime` raised `ValueError` — Python propagates it past
    /// the view, so the handler 500s (quirk 5).
    InvalidValue(SearchBug500),
}

/// `updated_at__gte` arm (`:2848`).
pub const SINCE_WHERE: &str = "\"issues\".\"updated_at\" >= :since";

/// Parse `?since=` with `django.utils.dateparse.parse_datetime` semantics
/// (Django 4.2.30 on CPython 3.12, probed — every rule below has a vector
/// in the tests): `datetime.fromisoformat` first, `datetime_re` fallback;
/// `None` → 400, `ValueError` → 500. Naive results attach UTC
/// (`USE_TZ = True`, `TIME_ZONE = "UTC"`, `settings/common.py:361-362` —
/// the same convention the naive `__gt` datetimes get).
pub fn parse_since(raw: Option<&str>) -> SinceOutcome {
    let Some(text) = raw else {
        return SinceOutcome::NoFilter;
    };
    if text.is_empty() {
        return SinceOutcome::NoFilter;
    }
    if let Some(found) = parse_fromiso_subset(text) {
        return match to_utc(found) {
            Some(at) => SinceOutcome::At(at),
            None => SinceOutcome::InvalidValue(SearchBug500::BadSinceValue),
        };
    }
    match parse_datetimere(text) {
        Datetimere::NoMatch => SinceOutcome::BadFormat(ParamError::error(SINCE_ERROR_MESSAGE)),
        Datetimere::Invalid => SinceOutcome::InvalidValue(SearchBug500::BadSinceValue),
        Datetimere::Found(found) => match to_utc(found) {
            Some(at) => SinceOutcome::At(at),
            None => SinceOutcome::InvalidValue(SearchBug500::BadSinceValue),
        },
    }
}

/// A parsed datetime: naive wall time plus optional UTC offset in
/// microseconds (fractional-second offsets exist — see quirk 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CandidateDt {
    naive: NaiveDateTime,
    offset_us: Option<i64>,
}

/// Combine into UTC. `None` is unreachable for years 1–9999 (the value
/// range fits `i64` microseconds with orders of magnitude to spare) and
/// maps to the 500 marker rather than a silent wrong instant.
fn to_utc(found: CandidateDt) -> Option<DateTime<Utc>> {
    let secs = found.naive.and_utc().timestamp() as i128 * 1_000_000
        + i128::from(found.naive.and_utc().timestamp_subsec_micros());
    let us = secs - i128::from(found.offset_us.unwrap_or(0));
    let us = i64::try_from(us).ok()?;
    DateTime::from_timestamp_micros(us)
}

/// Parse exactly `len` ASCII digits from the front of `bytes`.
fn take_digits(bytes: &[u8], len: usize) -> Option<u32> {
    let head = bytes.get(..len)?;
    if !head.iter().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    // ASCII digits never fail `u32` parsing at these widths.
    std::str::from_utf8(head).ok()?.parse().ok()
}

/// Length of the leading ASCII-digit run.
fn run_len(text: &str) -> usize {
    text.bytes().take_while(u8::is_ascii_digit).count()
}

/// `datetime.fromisoformat` (CPython 3.12) subset that `datetime_re` cannot
/// also match. Shapes: `YYYY-MM-DD | YYYYMMDD` plus week dates
/// (`YYYY-Www[-D] | YYYYWww[D]`, weekday optional — Monday when absent),
/// date-only (midnight), any single-char date/time separator, extended or
/// basic times (`HH[:MM[:SS]]`, fraction always means fractional seconds),
/// `Z`/numeric offsets with 0–1 whitespace chars before them, one ignored
/// junk char tolerated immediately before a zone, and one
/// ignored-or-applied trailing fraction after a numeric offset (quirk 6).
/// Anything unrecognized returns `None` so the caller falls through to
/// [`parse_datetimere`] — including out-of-range values, exactly like
/// `fromisoformat`'s `ValueError` falling through to the regex.
fn parse_fromiso_subset(text: &str) -> Option<CandidateDt> {
    // Byte-indexed throughout (`fromisoformat` is ASCII-strict, so any
    // non-ASCII byte fails the shape — except the separator, which is one
    // Unicode char by design).
    let bytes = text.as_bytes();
    let year = take_digits(bytes, 4)? as i32;
    // Python datetimes live in years 1–9999; chrono would also accept
    // year 0, so reject it here (the regex layer then decides 500).
    if year < 1 {
        return None;
    }
    // ISO week dates peel off first: their weekday is optional (Monday
    // when absent) and `-` doubles as the date/time separator.
    if bytes.len() >= 8 && bytes[4] == b'-' && bytes.get(5) == Some(&b'W') {
        return parse_extended_week(text, year);
    }
    if bytes.len() >= 7 && bytes[4] == b'W' {
        return parse_basic_week(text, year);
    }
    // Date part: extended or basic calendar.
    let (date, rest) = if bytes.len() >= 10 && bytes[4] == b'-' {
        // `YYYY-MM-DD`.
        if bytes.get(7) != Some(&b'-') {
            return None;
        }
        let month = take_digits(bytes.get(5..)?, 2)?;
        let day = take_digits(bytes.get(8..)?, 2)?;
        (NaiveDate::from_ymd_opt(year, month, day)?, text.get(10..)?)
    } else {
        // `YYYYMMDD`.
        let month = take_digits(bytes.get(4..)?, 2)?;
        let day = take_digits(bytes.get(6..)?, 2)?;
        (NaiveDate::from_ymd_opt(year, month, day)?, text.get(8..)?)
    };
    finish_date(date, rest)
}

/// Monday of an ISO week (`None` for week 0 / week 53 in a short year).
fn monday_of(year: i32, week: u32) -> Option<NaiveDate> {
    NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::Mon)
}

/// Shared tail after a complete date: empty means midnight, else exactly
/// one separator char — any of them (`T`, `t`, space, `X`, even `+`/`-`,
/// which is why `2025-01-01+05:30` reads as a time) — then the time.
/// A consumed separator enables the lone-digit drop in the time parser.
fn finish_date(date: NaiveDate, rest: &str) -> Option<CandidateDt> {
    if rest.is_empty() {
        // Date-only means midnight.
        return Some(CandidateDt {
            naive: date.and_hms_opt(0, 0, 0)?,
            offset_us: None,
        });
    }
    let sep_len = rest.chars().next()?.len_utf8();
    parse_fromiso_time(date, rest.get(sep_len..)?, true)
}

/// Extended ISO week date (`YYYY-Www` plus its optional tail). The weekday
/// defaults to Monday when absent (`2025-W01`, `2025-W01T00`). After `-`,
/// the weekday digit is taken only when NOT followed by another digit —
/// otherwise `-` is the date/time separator and the digits from the slot
/// are basic time (`2025-W01-12` → Mon 12:00, `2025-W01-112` fails).
/// Without the dash, any first char — even a digit — is the separator
/// (`2025-W011234+00` → Mon 23:00 via sep `1`). All probed against 3.12
/// `fromisoformat`.
fn parse_extended_week(text: &str, year: i32) -> Option<CandidateDt> {
    let week = take_digits(text.as_bytes().get(6..)?, 2)?;
    let tail = text.get(8..)?;
    if tail.is_empty() {
        let date = monday_of(year, week)?;
        return Some(CandidateDt {
            naive: date.and_hms_opt(0, 0, 0)?,
            offset_us: None,
        });
    }
    if let Some(after_dash) = tail.strip_prefix('-') {
        let slot = after_dash.as_bytes().first().copied();
        let slot_next_is_digit = after_dash.as_bytes().get(1).is_some_and(u8::is_ascii_digit);
        if slot.is_some_and(|byte| byte.is_ascii_digit()) && !slot_next_is_digit {
            let weekday = slot.unwrap_or(b'0') - b'0';
            if weekday == 0 || weekday > 7 {
                return None;
            }
            let date = NaiveDate::from_isoywd_opt(
                year,
                week,
                chrono::Weekday::try_from(weekday - 1).ok()?,
            )?;
            return finish_date(date, after_dash.get(1..)?);
        }
        let date = monday_of(year, week)?;
        // The `-` counts as the consumed separator (digit drop allowed:
        // `2025-W01-000+00` → :00).
        return parse_fromiso_time(date, after_dash, true);
    }
    let date = monday_of(year, week)?;
    finish_date(date, tail)
}

/// Basic ISO week date (`YYYYWww` plus its optional tail). With no slot
/// char, or a non-digit one, the weekday defaults to Monday and the
/// normal separator path applies (`2025W01`, `2025W01T00`). An invalid
/// weekday digit (0/8/9) is skipped and the rest parses as strict time
/// (`2025W01005+00` → 05:00). A valid weekday followed by digits tries
/// separator-less time first, then re-reads the first digit as a
/// separator (`2025W01112` → 12:00, `2025W01112345+00` → 23:45); both
/// attempts are strict — no lone-digit drop (`2025W011234+00` fails).
/// All probed.
fn parse_basic_week(text: &str, year: i32) -> Option<CandidateDt> {
    let bytes = text.as_bytes();
    let week = take_digits(bytes.get(5..)?, 2)?;
    let Some(slot) = bytes.get(7).copied() else {
        let date = monday_of(year, week)?;
        return Some(CandidateDt {
            naive: date.and_hms_opt(0, 0, 0)?,
            offset_us: None,
        });
    };
    if !slot.is_ascii_digit() {
        let date = monday_of(year, week)?;
        return finish_date(date, text.get(7..)?);
    }
    let weekday = slot - b'0';
    if weekday == 0 || weekday > 7 {
        // Invalid weekday digit: skip it, strict time on the rest.
        let date = monday_of(year, week)?;
        return parse_fromiso_time(date, text.get(8..)?, false);
    }
    let date =
        NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::try_from(weekday - 1).ok()?)?;
    let rest = text.get(8..)?;
    if rest.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        // Separator-less time first, then first-digit-as-separator; both
        // strict (no digit drop — the digit was never a separator).
        if let Some(found) = parse_fromiso_time(date, rest, false) {
            return Some(found);
        }
        // `rest[0]` is an ASCII digit, so byte 1 is a char boundary.
        return parse_fromiso_time(date, rest.get(1..)?, false);
    }
    finish_date(date, rest)
}

/// Whether two ASCII digits stand at `bytes[index..]`.
fn has_2_digits(bytes: &[u8], index: usize) -> bool {
    bytes
        .get(index..)
        .is_some_and(|tail| tail.len() >= 2 && tail[0].is_ascii_digit() && tail[1].is_ascii_digit())
}

/// Consume a `run` (> 0) digit fraction from the front of `tail`: the
/// first 6 digits kept (right-padded), the rest dropped. Six or more
/// digits switch the tail to scan mode — everything up to the first
/// `Z`/`+`/`-` is ignored, then the zone parses strictly from there
/// (first leader wins, no leader fails); fewer stay strict. Returns
/// `(microseconds, rest_after)`.
fn take_fraction(tail: &str, run: usize) -> Option<(u32, &str)> {
    let mut padded = tail[..run.min(6)].to_owned();
    while padded.len() < 6 {
        padded.push('0');
    }
    let micro: u32 = padded.parse().ok()?;
    let mut rest = tail.get(run..)?;
    if run >= 6 && !rest.is_empty() {
        let skip = rest
            .bytes()
            .position(|byte| matches!(byte, b'Z' | b'+' | b'-'))?;
        rest = rest.get(skip..)?;
    }
    Some((micro, rest))
}

/// Time + optional zone after the separator. `HH[:MM[:SS]]` (a `:` not
/// followed by 2 digits is not a component separator — it falls through
/// to the skip/zone tail) or basic `HH[MM[SS]]` (trailing digits after a
/// complete SS are a separator-less fraction). The fraction separators
/// are `[.,]`, plus `:` after an extended SS only; a bare separator with
/// no digits is left for the single-char skip. Fractions always mean
/// fractional *seconds*; 6+ digits switch the tail to scan-to-zone mode.
/// One ignored junk char is tolerated immediately before a zone, plus
/// optional single whitespace, then `Z` or a numeric offset with an
/// optional trailing fraction. `allow_digit_drop` is false only for time
/// reached without consuming a separator (basic-week digit tails), where
/// a lone digit drops only after a consumed component colon.
fn parse_fromiso_time(
    date: NaiveDate,
    time_text: &str,
    allow_digit_drop: bool,
) -> Option<CandidateDt> {
    // Cursor over bytes; every advance is bounds-checked, so non-ASCII
    // bytes fail the shape instead of panicking a slice.
    let bytes = time_text.as_bytes();
    let hour = take_digits(bytes, 2)?;
    let mut pos = 2;
    let mut minute: u32 = 0;
    let mut second: u32 = 0;
    // `:` joins the fraction separators (extended SS consumed).
    let mut colon_frac = false;
    // Bare digits are a fraction (basic SS consumed).
    let mut digit_frac = false;
    // A consumed component colon also enables the lone-digit drop.
    let mut colon_seen = false;
    if bytes.get(pos) == Some(&b':') {
        // Extended `HH:MM[:SS]` — but a `:` NOT followed by 2 digits is
        // not a component separator at all (`T00:+00` skips the colon to
        // the zone, `T00:5+00` fails — probed).
        if has_2_digits(bytes, pos + 1) {
            minute = take_digits(bytes.get(pos + 1..)?, 2)?;
            pos += 3;
            colon_seen = true;
            if bytes.get(pos) == Some(&b':') && has_2_digits(bytes, pos + 1) {
                second = take_digits(bytes.get(pos + 1..)?, 2)?;
                pos += 3;
                colon_frac = true;
            }
        }
    } else if let Some(tail) = bytes.get(pos..) {
        // Basic `HHMM[SS]` (or bare `HH` when fewer digits follow).
        if tail.len() >= 2 && tail[..2].iter().all(|byte| byte.is_ascii_digit()) {
            minute = take_digits(tail, 2)?;
            pos += 2;
            if let Some(tail) = bytes.get(pos..) {
                if tail.len() >= 2 && tail[..2].iter().all(|byte| byte.is_ascii_digit()) {
                    second = take_digits(tail, 2)?;
                    pos += 2;
                    digit_frac = true;
                }
            }
        }
    }
    let mut rest = time_text.get(pos..)?;
    // A lone digit drops only with a consumed separator behind it — or a
    // consumed component colon (`T00:005+00` → :00).
    let drop_allowed = allow_digit_drop || colon_seen;
    // Fraction: period or comma (always), colon (extended SS only), or
    // bare digits (basic SS only, 2+ of them — a single digit falls to
    // the skip rule: `T0000001+00` → :00; without a separator the run
    // must also be even). It always means fractional *seconds* (`T01.5`
    // → `01:00:00.5`, `T00:00:00:50` → `.50`).
    let mut micro: u32 = 0;
    let mut saw_fraction_sep = false;
    let seps: &[char] = if colon_frac {
        &['.', ',', ':']
    } else {
        &['.', ',']
    };
    if let Some(tail) = rest.strip_prefix(seps) {
        // A bare separator with no digits is left for the single-char
        // skip below (`,Z` / `:+00` parse with fraction 0; a trailing
        // `,` fails).
        let run = run_len(tail);
        if run > 0 {
            saw_fraction_sep = true;
            (micro, rest) = take_fraction(tail, run)?;
        }
    } else if digit_frac {
        let run = run_len(rest);
        if run >= 2 && (drop_allowed || run.is_multiple_of(2)) {
            saw_fraction_sep = true;
            (micro, rest) = take_fraction(rest, run)?;
        }
    }
    let naive = date.and_time(NaiveTime::from_hms_micro_opt(hour, minute, second, micro)?);
    if rest.is_empty() {
        return Some(CandidateDt {
            naive,
            offset_us: None,
        });
    }
    // A single ASCII char (except the zone leaders `Z`/`+`/`-`)
    // immediately before a valid zone is ignored (`T234+00` → 23:00,
    // `T12:30:45x+00`, `T12\x0b+00`, bare `,Z`; non-ASCII, doubled junk,
    // `++00`, and anything with a gap (`T234 +00`) or a fraction
    // (`T234.5+00`, `.5x+00`) all fail — all probed). Only without a
    // fraction, and checked before the whitespace strip so the zone must
    // abut the skipped char. Digits drop only with a consumed separator
    // or component colon behind them (`drop_allowed`).
    if !saw_fraction_sep {
        if let Some(tail) = rest.strip_prefix(|c: char| {
            c.is_ascii() && !matches!(c, 'Z' | '+' | '-') && (drop_allowed || !c.is_ascii_digit())
        }) {
            if parse_fromiso_zone(tail).is_some() {
                rest = tail;
            }
        }
        // Zero or one ASCII-whitespace char before the zone (space/tab
        // probed; two spaces fail through to `None`). With a fraction,
        // `fromisoformat` goes straight to the zone (`.5 +00:00` fails
        // there; extended shapes are rescued by the regex layer's `\s*`
        // instead).
        if let Some(first) = rest.chars().next() {
            if first.is_ascii_whitespace() {
                rest = rest.get(first.len_utf8()..)?;
            }
        }
    }
    let offset_us = parse_fromiso_zone(rest)?;
    Some(CandidateDt { naive, offset_us })
}

/// `Z` or a numeric zone (`±HH[:MM[:SS]]`, `±HHMM[SS]`), plus the optional
/// trailing fraction (quirk 6). Returns the offset in microseconds;
/// `Some(0)` for `Z`. Out-of-range (≥ 24h) or malformed zones fail so the
/// caller falls through to [`parse_datetimere`].
fn parse_fromiso_zone(text: &str) -> Option<Option<i64>> {
    if text == "Z" {
        return Some(Some(0));
    }
    let bytes = text.as_bytes();
    let (sign, mut pos) = match bytes.first() {
        Some(b'+') => (1i64, 1),
        Some(b'-') => (-1i64, 1),
        _ => return None,
    };
    let hours = i64::from(take_digits(bytes.get(pos..)?, 2)?);
    pos += 2;
    let mut minutes: i64 = 0;
    let mut seconds: i64 = 0;
    if bytes.get(pos) == Some(&b':') {
        minutes = i64::from(take_digits(bytes.get(pos + 1..)?, 2)?);
        pos += 3;
        if bytes.get(pos) == Some(&b':') {
            seconds = i64::from(take_digits(bytes.get(pos + 1..)?, 2)?);
            pos += 3;
        }
    } else if let Some(tail) = bytes.get(pos..) {
        // `±HHMM[SS]` without colons.
        if tail.len() >= 2 && tail[..2].iter().all(|byte| byte.is_ascii_digit()) {
            minutes = i64::from(take_digits(tail, 2)?);
            pos += 2;
            if let Some(tail) = bytes.get(pos..) {
                if tail.len() >= 2 && tail[..2].iter().all(|byte| byte.is_ascii_digit()) {
                    seconds = i64::from(take_digits(tail, 2)?);
                    pos += 2;
                }
            }
        }
    }
    // Overflow normalizes (`+00:61` → `+01:01`); ≥ 24h fails the layer.
    let total_secs = hours * 3600 + minutes * 60 + seconds;
    if total_secs >= 86_400 {
        return None;
    }
    let mut offset_us = sign * total_secs * 1_000_000;
    let mut rest = text.get(pos..)?;
    if let Some(tail) = rest.strip_prefix(['.', ',']) {
        let run = run_len(tail);
        if run == 0 {
            return None;
        }
        rest = tail.get(run..)?;
        if !rest.is_empty() {
            // A second fraction (or any other trailing junk) fails.
            return None;
        }
        if total_secs != 0 {
            // A nonzero offset absorbs the fraction as fractional
            // seconds; a zero offset drops it (probed asymmetry).
            let mut padded = tail[..run.min(6)].to_owned();
            while padded.len() < 6 {
                padded.push('0');
            }
            let frac: i64 = padded.parse().ok()?;
            offset_us += sign * frac;
        }
    } else if !rest.is_empty() {
        return None;
    }
    Some(Some(offset_us))
}

/// `datetime_re` (`django/utils/dateparse.py`), transcribed with two
/// deliberate narrowings, both documented: `\d` is ASCII-only (`[0-9]` —
/// Python's Unicode decimal digits would need `to_digit` plumbing for an
/// input nobody sends; same posture as the merged `int()` ports), and the
/// trailing `$` is `(?:\n?)$` to reproduce Python's match-before-trailing-
/// newline semantics under the `regex` crate's end-only `$`.
static DATETIMERE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?P<year>[0-9]{4})-(?P<month>[0-9]{1,2})-(?P<day>[0-9]{1,2})[T ](?P<hour>[0-9]{1,2}):(?P<minute>[0-9]{1,2})(?::(?P<second>[0-9]{1,2})(?:[.,](?P<microsecond>[0-9]{1,6})[0-9]{0,6})?)?\s*(?P<tzinfo>Z|[+-][0-9]{2}(?::?[0-9]{2})?)?(?:\n?)$",
    )
    .expect("datetime_re transcribes")
});

/// Outcome of the `datetime_re` fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Datetimere {
    /// The regex did not match — `parse_datetime` returns `None` (400).
    NoMatch,
    /// The regex matched but the parts are not a valid datetime (or the
    /// offset is out of range) — `ValueError` propagates (500).
    Invalid,
    /// Parsed.
    Found(CandidateDt),
}

/// The regex fallback: full-string match (anchored — `match()` +
/// trailing `$`), microsecond `ljust(6, "0")`, `Z`/offset handling with
/// `get_fixed_timezone` range semantics (strictly inside ±24h).
fn parse_datetimere(text: &str) -> Datetimere {
    let Some(caps) = DATETIMERE.captures(text) else {
        return Datetimere::NoMatch;
    };
    // `captures` is unanchored at the start; `match()` semantics need the
    // match to begin at 0.
    if caps.get(0).is_none_or(|hit| hit.start() != 0) {
        return Datetimere::NoMatch;
    }
    let num = |name: &str| -> Option<u32> { caps.name(name)?.as_str().parse().ok() };
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute)) = (
        num("year"),
        num("month"),
        num("day"),
        num("hour"),
        num("minute"),
    ) else {
        return Datetimere::Invalid;
    };
    let second = num("second").unwrap_or(0);
    let micro = caps.name("microsecond").map_or(Ok(0), |hit| {
        let mut padded = hit.as_str().to_owned();
        while padded.len() < 6 {
            padded.push('0');
        }
        padded.parse::<u32>().map_err(|_| ())
    });
    let Ok(micro) = micro else {
        return Datetimere::Invalid;
    };
    let offset_us = match caps.name("tzinfo").map(|hit| hit.as_str()) {
        None => None,
        Some("Z") => Some(0),
        Some(tz) => {
            // `offset_mins = int(tzinfo[-2:]) if len(tzinfo) > 3 else 0`.
            let mins = if tz.len() > 3 {
                tz[tz.len() - 2..].parse::<i64>().unwrap_or(0)
            } else {
                0
            };
            let hours = tz[1..3].parse::<i64>().unwrap_or(0);
            let mut total = (hours * 60 + mins) * 60;
            if tz.starts_with('-') {
                total = -total;
            }
            // `get_fixed_timezone` rejects anything at/over ±24h.
            if total.abs() >= 86_400 {
                return Datetimere::Invalid;
            }
            Some(total * 1_000_000)
        }
    };
    // chrono accepts year 0; Python raises (`ValueError` → 500).
    if year < 1 {
        return Datetimere::Invalid;
    }
    let (Some(date), Some(time)) = (
        NaiveDate::from_ymd_opt(year as i32, month, day),
        NaiveTime::from_hms_micro_opt(hour, minute, second, micro),
    ) else {
        return Datetimere::Invalid;
    };
    Datetimere::Found(CandidateDt {
        naive: date.and_time(time),
        offset_us,
    })
}

// ---------------------------------------------------------------------------
// Advanced search: statement + result assembly
// ---------------------------------------------------------------------------

/// Assembled advanced-search filters (post-parse).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdvancedParts<'a> {
    /// The stripped non-empty query.
    pub query: &'a str,
    /// A UUID-form `?project=` applies (`AND project_id`).
    /// Identifier-form projects resolve first (handler); a miss 404s
    /// before any statement runs.
    pub project_uuid_filter: bool,
    /// Parsed `?status=`.
    pub status: StatusFilter,
    /// A valid `?since=` applies (`AND updated_at >= :since`).
    pub since_filter: bool,
    /// Parsed `?sort=`.
    pub sort: Sort,
}

/// Full advanced statement. Filter arms join in source application order
/// (project → status → since → FTS with `include_comments=True`,
/// `with_rank`/`with_headline` selected above). `:limit` binds
/// [`advanced_limit`]'s `1..=50` value. The handler only calls this with
/// the stripped non-empty query ([`parse_advanced_query`]); an empty one
/// yields the base statement without FTS arms, never executed.
pub fn advanced_search_sql(parts: &AdvancedParts<'_>) -> String {
    let mut chain = search_base_where();
    if parts.project_uuid_filter {
        chain.push_str(" AND ");
        chain.push_str(PROJECT_UUID_WHERE);
    }
    if let Some(status) = status_where_sql(parts.status) {
        chain.push_str(" AND ");
        chain.push_str(&status);
    }
    if parts.since_filter {
        chain.push_str(" AND ");
        chain.push_str(SINCE_WHERE);
    }
    if let Some(fts_where) = search_fts_where(parts.query, true) {
        chain.push_str(" AND ");
        chain.push_str(&fts_where);
    }
    format!(
        "SELECT DISTINCT {} FROM \"issues\" {} WHERE ({}) ORDER BY {} LIMIT :limit",
        advanced_select_sql(),
        SEARCH_JOINS_SQL,
        chain,
        sort_order_sql(parts.sort),
    )
}

/// One `values()` row for the manual result assembly (`:2860-2875`).
/// Datetimes and UUIDs arrive pre-rendered (DRF rendering is owned by the
/// handler layer with the shared kernels; this struct pins the assembly
/// shape and key order, which is what the view owns).
#[derive(Debug, Clone, PartialEq)]
pub struct AdvancedRow<'a> {
    /// `str(row["id"])`.
    pub id: &'a str,
    /// `row["sequence_id"]` (int4).
    pub sequence_id: i32,
    /// `row["name"]`.
    pub name: &'a str,
    /// `row["_headline"]` (`NULL` when the description is empty).
    pub headline: Option<&'a str>,
    /// `row["_rank"]` (`NULL` only in theory; `or 0.0` covers it).
    pub rank: Option<f64>,
    /// DRF-rendered `row["created_at"]`.
    pub created_at: &'a str,
    /// DRF-rendered `row["updated_at"]`.
    pub updated_at: &'a str,
    /// DRF-rendered `row["completed_at"]` (`None` renders `null`).
    pub completed_at: Option<&'a str>,
    /// `row["state__name"]` (`None` when stateless — always present as a
    /// key, `None`-filled).
    pub state_name: Option<&'a str>,
    /// `row["state__group"]` (same).
    pub state_group: Option<&'a str>,
    /// `str(row["project_id"])`.
    pub project_id: &'a str,
    /// `row["project__identifier"]`.
    pub project_identifier: &'a str,
    /// `row["project__name"]`.
    pub project_name: &'a str,
    /// `row["workspace__slug"]`.
    pub workspace_slug: &'a str,
}

/// `f"{project__identifier}-{sequence_id}"` (`:2883`).
pub fn compose_identifier(project_identifier: &str, sequence_id: i32) -> String {
    format!("{project_identifier}-{sequence_id}")
}

fn opt_str(value: Option<&str>) -> Value {
    match value {
        Some(text) => Value::String(text.to_owned()),
        None => Value::Null,
    }
}

/// One manual result dict (`:2880-2910`) — built in the view, *not* via
/// `IssueAdvancedSearchResultSerializer` (fixture-pinned). Key order is
/// source order; `url` is appended only when the web base URL resolves
/// (`issue_web_url` returns `None` otherwise — the handler owns that
/// lookup). `rank` is `float(row["_rank"] or 0.0)`.
pub fn advanced_result(row: &AdvancedRow<'_>, url: Option<&str>) -> Value {
    let mut state = Map::with_capacity(2);
    state.insert("name".to_owned(), opt_str(row.state_name));
    state.insert("group".to_owned(), opt_str(row.state_group));
    let mut project = Map::with_capacity(3);
    project.insert("id".to_owned(), Value::String(row.project_id.to_owned()));
    project.insert(
        "identifier".to_owned(),
        Value::String(row.project_identifier.to_owned()),
    );
    project.insert(
        "name".to_owned(),
        Value::String(row.project_name.to_owned()),
    );
    let mut out = Map::with_capacity(13);
    out.insert("id".to_owned(), Value::String(row.id.to_owned()));
    out.insert(
        "sequence_id".to_owned(),
        Value::Number(row.sequence_id.into()),
    );
    out.insert(
        "identifier".to_owned(),
        Value::String(compose_identifier(row.project_identifier, row.sequence_id)),
    );
    out.insert("name".to_owned(), Value::String(row.name.to_owned()));
    out.insert(
        "snippet".to_owned(),
        Value::String(fts::extract_snippet(row.headline)),
    );
    out.insert("state".to_owned(), Value::Object(state));
    out.insert("project".to_owned(), Value::Object(project));
    out.insert(
        "workspace_slug".to_owned(),
        Value::String(row.workspace_slug.to_owned()),
    );
    out.insert(
        "created_at".to_owned(),
        Value::String(row.created_at.to_owned()),
    );
    out.insert(
        "updated_at".to_owned(),
        Value::String(row.updated_at.to_owned()),
    );
    out.insert("completed_at".to_owned(), opt_str(row.completed_at));
    out.insert("rank".to_owned(), Value::from(row.rank.unwrap_or(0.0)));
    if let Some(url) = url {
        out.insert("url".to_owned(), Value::String(url.to_owned()));
    }
    Value::Object(out)
}

/// Success envelope (`:2912-2915`): `{"query", "count", "results"}`.
pub fn advanced_envelope(query: &str, results: Vec<Value>) -> Value {
    let mut out = Map::with_capacity(3);
    out.insert("query".to_owned(), Value::String(query.to_owned()));
    out.insert("count".to_owned(), Value::Number(results.len().into()));
    out.insert("results".to_owned(), Value::Array(results));
    Value::Object(out)
}

// ---------------------------------------------------------------------------
// Page reads: BasePageReadAPIEndpoint (views/page.py:173-232, 480-553)
// ---------------------------------------------------------------------------

/// `Page.PUBLIC_ACCESS` (`db/models/page.py:25`).
pub const PAGE_PUBLIC_ACCESS: i32 = 0;

/// `select_related("workspace")` + `select_related("owned_by")`
/// (`:195-196`), in fixture order. Both FKs are non-nullable, so Django
/// renders `INNER JOIN` (a nullable FK would be `LEFT OUTER JOIN`).
pub fn page_joins_sql() -> String {
    "INNER JOIN \"workspaces\" ON (\"pages\".\"workspace_id\" = \"workspaces\".\"id\") INNER JOIN \"users\" ON (\"pages\".\"owned_by_id\" = \"users\".\"id\")".to_owned()
}

/// The `Exists(linked_to_project)` subquery (`:184-190`): one `Exists`
/// over the through table rather than a join through the `projects` m2m,
/// so the row never fans out and the project filters cannot match on
/// *different* projects. Aliases (`U0`/`U2`/`U3`) and the `LIMIT 1`
/// (no `ORDER BY` — `Exists` clears ordering) are compiler-pinned by the
/// fixture. `:member_id` binds the request user, `:project_id` the URL
/// `project_id` kwarg.
pub fn linked_to_project_exists_sql() -> String {
    "EXISTS(SELECT 1 AS \"a\" FROM \"project_pages\" U0 INNER JOIN \"projects\" U2 ON (U0.\"project_id\" = U2.\"id\") INNER JOIN \"project_members\" U3 ON (U2.\"id\" = U3.\"project_id\") WHERE (U0.\"deleted_at\" IS NULL AND U0.\"page_id\" = (\"pages\".\"id\") AND U2.\"archived_at\" IS NULL AND U3.\"is_active\" AND U3.\"member_id\" = :member_id AND U0.\"project_id\" = :project_id) LIMIT 1)".to_owned()
}

/// `get_queryset` visibility `WHERE` (`:192-197`), in fixture order:
/// live pages, workspace slug, linked-to-project `Exists`, then
/// public-or-owned (`Q(access=PUBLIC) | Q(owned_by=user)` — someone
/// else's private page vanishes from the queryset and reads as 404).
pub fn page_visibility_where() -> String {
    format!(
        "\"pages\".\"deleted_at\" IS NULL AND \"workspaces\".\"slug\" = :slug AND {} AND (\"pages\".\"access\" = {} OR \"pages\".\"owned_by_id\" = :member_id)",
        linked_to_project_exists_sql(),
        PAGE_PUBLIC_ACCESS,
    )
}

/// `Page.Meta.ordering` (`db/models/page.py:64`).
pub const PAGE_META_ORDER: &str = "\"pages\".\"created_at\" DESC";

/// Visibility + pk arm shared by the single-page fetches.
fn page_pk_where() -> String {
    format!(
        "{} AND \"pages\".\"id\" = :page_id",
        page_visibility_where()
    )
}

/// `validate_parent`'s parent fetch and `get_page_or_error`
/// (`:208,:223`): `get_queryset().filter(pk=…).first()` — `Meta`
/// ordering kept, `LIMIT 1`. `columns` is the handler's projection (the
/// db layer owns the full column lists); the row identity is what this
/// module pins.
pub fn page_fetch_first_sql(columns: &str) -> String {
    format!(
        "SELECT {} FROM \"pages\" {} WHERE ({}) ORDER BY {} LIMIT 1",
        columns,
        page_joins_sql(),
        page_pk_where(),
        PAGE_META_ORDER,
    )
}

/// `detail_response` (`:228-230`): `get_queryset().get(pk=…)` — `.get()`
/// *clears* ordering and applies `LIMIT 21` (`MAX_GET_RESULTS`,
/// `django/db/models/query.py:40,615-649`, verified in source).
pub fn page_detail_get_sql(columns: &str) -> String {
    format!(
        "SELECT {} FROM \"pages\" {} WHERE ({}) LIMIT 21",
        columns,
        page_joins_sql(),
        page_pk_where(),
    )
}

/// Minimal `validate_parent` parent lookup: only `id` + `archived_at`
/// are consumed (`:208-211`), so the projection narrows to those two
/// (Django selects the full `select_related` row; same row either way).
pub fn parent_fetch_sql() -> String {
    page_fetch_first_sql("\"pages\".\"id\", \"pages\".\"archived_at\"")
}

/// One ancestor-walk read (`:219`):
/// `Page.objects.filter(pk=…).values_list("parent_id", flat=True).first()`
/// — global (unscoped, soft-delete scope only), `Meta` ordering,
/// `LIMIT 1`. Returns the parent id or no row (a `NULL` parent and a
/// missing row both read as no row — `.first()`).
pub fn parent_lookup_sql() -> &'static str {
    "SELECT \"pages\".\"parent_id\" FROM \"pages\" WHERE (\"pages\".\"deleted_at\" IS NULL AND \"pages\".\"id\" = :page_id) ORDER BY \"pages\".\"created_at\" DESC LIMIT 1"
}

/// What the parent lookup found (`:206-211`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentLookup {
    /// `parent_id is None` — always fine.
    Absent,
    /// A given id no visible row carries.
    Missing,
    /// A visible row.
    Found(ParentRef),
}

/// The two consumed parent columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentRef {
    /// `parent.id`.
    pub id: Uuid,
    /// `parent.archived_at is not None`.
    pub archived: bool,
}

/// `validate_parent` failures (`:199-220`), all 400.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidateParentError {
    /// `"Parent page not found in this project"`.
    NotFound,
    /// `"Parent page is archived"`.
    Archived,
    /// `"A page cannot be nested under itself"` (the page itself *or* one
    /// of its descendants — the walk reports both with this one string).
    SelfNest,
}

impl ValidateParentError {
    /// The 400 body, via the shared view-inline envelope.
    pub fn body(&self) -> String {
        let message = match self {
            ValidateParentError::NotFound => "Parent page not found in this project",
            ValidateParentError::Archived => "Parent page is archived",
            ValidateParentError::SelfNest => "A page cannot be nested under itself",
        };
        ParamError::error(message).body()
    }
}

/// `validate_parent` (`:199-220`). `page_id` is `Some` on update
/// (cycle check) and `None` on create. `fetch_parent` runs
/// [`parent_lookup_sql`] and returns the parent id, or `None` when the
/// row is missing *or* its parent is `NULL` (both read as `.first()` →
/// `None`, ending the walk silently — quirk 11). The walk itself is
/// unscoped (`Page.objects`, not the queryset) and cycle-guarded by the
/// `seen` set.
pub fn validate_parent(
    lookup: ParentLookup,
    page_id: Option<Uuid>,
    fetch_parent: &dyn Fn(Uuid) -> Option<Uuid>,
) -> Option<ValidateParentError> {
    let found = match lookup {
        ParentLookup::Absent => return None,
        ParentLookup::Missing => return Some(ValidateParentError::NotFound),
        ParentLookup::Found(found) => found,
    };
    if found.archived {
        return Some(ValidateParentError::Archived);
    }
    let page_id = page_id?;
    let mut ancestor = found.id;
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(ancestor) {
            return None;
        }
        if ancestor == page_id {
            return Some(ValidateParentError::SelfNest);
        }
        ancestor = fetch_parent(ancestor)?;
    }
}

/// `get_page_or_error` miss body (`:225`), 404.
pub fn page_not_found_body() -> String {
    ParamError::error("Page not found").body()
}

/// `get_page_or_error` miss status (`:225`).
pub const PAGE_NOT_FOUND_STATUS: u16 = 404;

// ---------------------------------------------------------------------------
// Page archive read path (views/page.py:480-553)
// ---------------------------------------------------------------------------

/// `ROLE.ADMIN.value` (`db/models/project.py:28-30`).
pub const PROJECT_ROLE_ADMIN: i32 = 20;

/// The `_check_can_archive` admin probe (`:494-496`):
/// `ProjectMember.objects.filter(project_id, member, is_active, role=20)`
/// `.exists()` — `SELECT (1) … LIMIT 1`, no `ORDER BY`. The guard
/// *decision* (locked / non-owner / admin → status + body) is
/// PIDASHCONV-671's (`perms.rs`); this is the read it evaluates.
pub fn archive_admin_exists_sql() -> String {
    format!(
        "SELECT (1) AS \"a\" FROM \"project_members\" WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"project_members\".\"project_id\" = :project_id AND \"project_members\".\"member_id\" = :member_id AND \"project_members\".\"is_active\" AND \"project_members\".\"role\" = {}) LIMIT 1",
        PROJECT_ROLE_ADMIN,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const F18_08: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/v1_work_items/queries/F18-08.search_page.json"
    );

    fn fixture() -> Value {
        let raw = std::fs::read_to_string(F18_08).expect("F18-08 fixture exists");
        serde_json::from_str(&raw).expect("F18-08 fixture is valid JSON")
    }

    fn unit<'a>(fx: &'a Value, unit: &str) -> &'a Value {
        fx.pointer(&format!("/units/{unit}"))
            .unwrap_or_else(|| panic!("F18-08 lacks units.{unit}"))
    }

    /// Skeleton comparison: lowercase, dequoted, whitespace-collapsed.
    /// Fixture SQL carries live values where builders emit placeholders,
    /// so callers map one side before comparing (never both).
    fn norm(sql: &str) -> String {
        sql.to_lowercase()
            .replace('"', "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    // ------------------------------------------------------------------
    // Shared search base
    // ------------------------------------------------------------------

    #[test]
    fn joins_match_fixture_both_endpoints() {
        let fx = fixture();
        for name in ["legacy_search_queryset", "advanced_search_queryset"] {
            let sql = unit(&fx, name)["sql"].as_str().expect("sql recorded");
            assert!(
                norm(sql).contains(&norm(SEARCH_JOINS_SQL)),
                "{name} joins diverge"
            );
        }
    }

    #[test]
    fn manager_where_pinned() {
        // The triage arm embeds an AND, so pin the whole predicate.
        assert_eq!(
            SEARCH_MANAGER_WHERE,
            "\"issues\".\"deleted_at\" IS NULL AND NOT (\"states\".\"group\" = 'triage' AND \"states\".\"group\" IS NOT NULL) AND NOT (\"issues\".\"archived_at\" IS NOT NULL) AND NOT (\"projects\".\"archived_at\" IS NOT NULL) AND NOT (\"issues\".\"is_draft\")"
        );
    }

    #[test]
    fn base_arms_all_present_in_legacy_fixture_in_order() {
        let fx = fixture();
        let sql = norm(unit(&fx, "legacy_search_queryset")["sql"].as_str().unwrap());
        // Map placeholders to the recorded live values (fixture side is
        // unquoted `str(query)` rendering).
        let arms = [
            "\"issues\".\"deleted_at\" is null",
            "not (\"states\".\"group\" = triage and \"states\".\"group\" is not null)",
            "not (\"issues\".\"archived_at\" is not null)",
            "not (\"projects\".\"archived_at\" is not null)",
            "not (\"issues\".\"is_draft\")",
            "\"projects\".\"archived_at\" is null",
            "\"project_members\".\"is_active\"",
            "\"project_members\".\"member_id\" = 79c81d76-5a93-4d3d-894d-5935576834b6",
            "\"workspaces\".\"slug\" = ws-conv659-2",
        ];
        let mut cursor = 0;
        for arm in arms {
            let needle = norm(arm);
            let pos = sql[cursor..]
                .find(&needle)
                .unwrap_or_else(|| panic!("missing arm: {arm}"));
            cursor += pos + needle.len();
        }
    }

    #[test]
    fn search_fts_none_when_empty_some_otherwise() {
        assert_eq!(search_fts_where("", false), None);
        assert_eq!(search_fts_where("", true), None);
        // Whitespace-only applies (Python truthiness, not stripped).
        assert!(search_fts_where(" ", false).is_some());
        assert!(search_fts_where("auth", false).is_some());
    }

    #[test]
    fn search_filter_is_the_shared_fts_closure() {
        // Reuse proof: the chain is byte-identical to fts.rs output.
        assert_eq!(
            search_fts_where("auth", false).expect("applies"),
            fts::search_filter_sql(false, ":fts", ":seq", ":like")
        );
        assert_eq!(
            search_fts_where("auth", true).expect("applies"),
            fts::search_filter_sql(true, ":fts", ":seq", ":like")
        );
    }

    #[test]
    fn fts_arm_order_and_comment_gating() {
        let legacy = norm(&search_where("auth", false));
        for arm in [
            "@@ websearch_to_tsquery",
            "issues.name ilike",
            "issues.sequence_id in",
            "projects.identifier ilike",
        ] {
            assert!(legacy.contains(arm), "legacy lacks {arm}");
        }
        assert!(
            !legacy.contains("issue_comments"),
            "legacy must not widen to comments"
        );
        let advanced = norm(&search_where("auth", true));
        let positions: Vec<usize> = [
            "@@ websearch_to_tsquery",
            "issues.name ilike",
            "issues.id in (select issue_id",
            "issues.sequence_id in",
            "projects.identifier ilike",
        ]
        .iter()
        .map(|arm| advanced.find(arm).unwrap_or_else(|| panic!("lacks {arm}")))
        .collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "arm order broken: {advanced}"
        );
    }

    #[test]
    fn text_binds_derive_from_query() {
        let binds = search_text_binds("auth 424242");
        assert_eq!(binds.fts, "auth 424242");
        assert_eq!(binds.seq, vec![424242]);
        assert_eq!(binds.like, "auth 424242");
        let binds = search_text_binds("100%_\\");
        assert_eq!(binds.seq, vec![100]);
        assert_eq!(binds.like, "100\\%\\_\\\\");
    }

    // ------------------------------------------------------------------
    // Legacy search
    // ------------------------------------------------------------------

    #[test]
    fn legacy_values_keys_match_live_body() {
        let fx = fixture();
        let live = &unit(&fx, "legacy_search_live")["issues"][0];
        let mut keys: Vec<&str> = live
            .as_object()
            .expect("issue object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut expected = LEGACY_VALUES_KEYS.to_vec();
        expected.sort_unstable();
        assert_eq!(keys, expected);
        assert_eq!(
            LEGACY_VALUES_KEYS,
            &[
                "name",
                "id",
                "sequence_id",
                "project__identifier",
                "project_id",
                "workspace__slug"
            ]
        );
    }

    #[test]
    fn legacy_select_appends_created_at_extra() {
        let select = legacy_select_sql();
        assert_eq!(select.split(',').count(), 7);
        assert!(select.ends_with("\"issues\".\"created_at\""));
    }

    #[test]
    fn legacy_project_filter_vectors() {
        let uuid = "d715be3d-234f-46ef-89a3-97f0c7c04b7e";
        let id = Uuid::parse_str(uuid).unwrap();
        assert_eq!(legacy_project_filter(None, None), LegacyProject::None);
        assert_eq!(
            legacy_project_filter(Some("false"), None),
            LegacyProject::None
        );
        assert_eq!(
            legacy_project_filter(Some("false"), Some("")),
            LegacyProject::None
        );
        // Absent flag defaults to "false": a bare project_id filters.
        assert_eq!(
            legacy_project_filter(None, Some(uuid)),
            LegacyProject::Filter(id)
        );
        assert_eq!(
            legacy_project_filter(Some("false"), Some(uuid)),
            LegacyProject::Filter(id)
        );
        // Braced/URN forms filter on the same parsed value (the handler
        // binds canonical form, as Django's UUIDField does).
        assert_eq!(
            legacy_project_filter(
                Some("false"),
                Some("{d715be3d-234f-46ef-89a3-97f0c7c04b7e}")
            ),
            LegacyProject::Filter(id)
        );
        assert_eq!(
            legacy_project_filter(Some("false"), Some("bogus")),
            LegacyProject::InvalidUuid
        );
        // Anything but the exact string "false" searches workspace-wide.
        for flag in ["true", "", "False", "FALSE", " false", "0"] {
            assert_eq!(
                legacy_project_filter(Some(flag), Some(uuid)),
                LegacyProject::None,
                "flag {flag:?}"
            );
        }
    }

    #[test]
    fn legacy_limit_vectors() {
        assert_eq!(legacy_limit_sql(None).as_deref(), Ok("LIMIT 10"));
        assert_eq!(legacy_limit_sql(Some("10")).as_deref(), Ok("LIMIT 10"));
        assert_eq!(legacy_limit_sql(Some(" 12 ")).as_deref(), Ok("LIMIT 12"));
        assert_eq!(legacy_limit_sql(Some("+12")).as_deref(), Ok("LIMIT 12"));
        assert_eq!(legacy_limit_sql(Some("007")).as_deref(), Ok("LIMIT 7"));
        assert_eq!(legacy_limit_sql(Some("0")).as_deref(), Ok("LIMIT 0"));
        assert_eq!(legacy_limit_sql(Some("-0")).as_deref(), Ok("LIMIT 0"));
        assert_eq!(legacy_limit_sql(Some("1_0")).as_deref(), Ok("LIMIT 10"));
        // Unbounded precision survives into the literal (Postgres int8
        // overflow 500s, exactly like Django).
        assert_eq!(
            legacy_limit_sql(Some("99999999999999999999999999")).as_deref(),
            Ok("LIMIT 99999999999999999999999999")
        );
        assert_eq!(
            legacy_limit_sql(Some("-5")),
            Err(SearchBug500::NegativeLegacyLimit)
        );
        assert_eq!(
            legacy_limit_sql(Some("-99999999999999999999999999")),
            Err(SearchBug500::NegativeLegacyLimit)
        );
        for bad in ["", "abc", "12.5", "0x10", "12\n34"] {
            assert_eq!(
                legacy_limit_sql(Some(bad)),
                Err(SearchBug500::BadLegacyLimit),
                "input {bad:?}"
            );
        }
    }

    #[test]
    fn legacy_query_present_vectors() {
        assert!(!legacy_query_present(None));
        assert!(!legacy_query_present(Some("")));
        assert!(legacy_query_present(Some(" ")));
        assert!(legacy_query_present(Some("x")));
    }

    #[test]
    fn legacy_sql_replays_fixture() {
        let fx = fixture();
        let recorded = unit(&fx, "legacy_search_queryset")["sql"]
            .as_str()
            .expect("sql recorded");
        let built = legacy_search_sql("zxcvsearchtoken", LegacyProject::None, "LIMIT 10");
        // SELECT prefix: ordered, exact after normalization (no binds).
        let recorded_select = norm(recorded.split(" FROM ").next().unwrap());
        let built_select = norm(built.split(" FROM ").next().unwrap());
        assert_eq!(built_select, recorded_select);
        // Tail: Meta ordering + limit.
        assert!(
            norm(&built).ends_with("order by issues.created_at desc limit 10"),
            "tail: {built}"
        );
        assert!(
            norm(recorded).ends_with("order by issues.created_at desc limit 10"),
            "fixture tail moved?"
        );
        // No project filter arm without the filter (the join and the
        // select list still name the column).
        assert!(!built.contains(&format!("AND {PROJECT_UUID_WHERE}")));
    }

    #[test]
    fn legacy_project_sql_adds_arm() {
        let id = Uuid::parse_str("d715be3d-234f-46ef-89a3-97f0c7c04b7e").unwrap();
        let built = legacy_search_sql("x", LegacyProject::Filter(id), "LIMIT 10");
        assert!(built.contains(&format!("AND {PROJECT_UUID_WHERE}")));
    }

    #[test]
    fn empty_search_body_matches_fixture() {
        assert_eq!(EMPTY_SEARCH_BODY, "{\"issues\":[]}");
        let fx = fixture();
        let body = &unit(&fx, "legacy_search_live")["no_query"]["body"];
        assert_eq!(
            serde_json::from_str::<Value>(EMPTY_SEARCH_BODY).unwrap(),
            *body
        );
    }

    // ------------------------------------------------------------------
    // Advanced params: query / sort / limit / status / project
    // ------------------------------------------------------------------

    #[test]
    fn advanced_query_vectors() {
        assert_eq!(parse_advanced_query(None), None);
        assert_eq!(parse_advanced_query(Some("")), None);
        assert_eq!(parse_advanced_query(Some("   ")), None);
        assert_eq!(parse_advanced_query(Some("  x  ")), Some("x".to_owned()));
    }

    #[test]
    fn empty_advanced_body_matches_fixture() {
        assert_eq!(
            EMPTY_ADVANCED_BODY,
            "{\"query\":\"\",\"count\":0,\"results\":[]}"
        );
        let fx = fixture();
        let body = serde_json::from_str::<Value>(EMPTY_ADVANCED_BODY).unwrap();
        assert_eq!(
            body["query"],
            unit(&fx, "advanced_search_live")["no_q"]["query"]
        );
        assert_eq!(
            body["count"],
            unit(&fx, "advanced_search_live")["no_q"]["count"]
        );
        assert_eq!(
            body["results"],
            unit(&fx, "advanced_search_live")["no_q"]["results"]
        );
    }

    #[test]
    fn sort_vectors_and_error_body() {
        assert_eq!(parse_sort(None), Ok(Sort::Rank));
        // `(get("sort") or "rank")`: empty string also defaults to rank.
        assert_eq!(parse_sort(Some("")), Ok(Sort::Rank));
        assert_eq!(parse_sort(Some("rank")), Ok(Sort::Rank));
        assert_eq!(parse_sort(Some("RANK")), Ok(Sort::Rank));
        assert_eq!(parse_sort(Some("-created")), Ok(Sort::Created));
        assert_eq!(parse_sort(Some("-UPDATED")), Ok(Sort::Updated));
        let err = parse_sort(Some("bogus")).expect_err("bogus sorts 400");
        assert_eq!(err.key, "error");
        assert_eq!(
            err.body(),
            "{\"error\":\"Unknown sort 'bogus'. Valid: rank, -created, -updated.\"}"
        );
        // The echoed value is lowercased.
        let err = parse_sort(Some("BOGUS")).expect_err("400");
        assert!(err.body().contains("Unknown sort 'bogus'"));
        // Fixture pins the same body.
        let fx = fixture();
        let pinned = &unit(&fx, "advanced_search_live")["bad_sort"]["body"]["error"];
        assert_eq!(
            serde_json::from_str::<Value>(&err.body()).unwrap()["error"],
            serde_json::Value::String(
                "Unknown sort 'bogus'. Valid: rank, -created, -updated.".to_owned()
            )
        );
        assert_eq!(
            pinned.as_str().unwrap(),
            "Unknown sort 'bogus'. Valid: rank, -created, -updated."
        );
    }

    #[test]
    fn sort_order_shapes() {
        assert_eq!(
            sort_order_sql(Sort::Rank),
            "\"_rank\" DESC, \"issues\".\"created_at\" DESC"
        );
        assert_eq!(
            sort_order_sql(Sort::Created),
            "\"issues\".\"created_at\" DESC"
        );
        assert_eq!(
            sort_order_sql(Sort::Updated),
            "\"issues\".\"updated_at\" DESC"
        );
    }

    #[test]
    fn advanced_limit_vectors() {
        assert_eq!(advanced_limit(None), 10);
        assert_eq!(advanced_limit(Some("12")), 12);
        assert_eq!(advanced_limit(Some(" 12 ")), 12);
        assert_eq!(advanced_limit(Some("+12")), 12);
        assert_eq!(advanced_limit(Some("1_0")), 10);
        assert_eq!(advanced_limit(Some("1")), 1);
        assert_eq!(advanced_limit(Some("50")), 50);
        // Clamp, not error.
        assert_eq!(advanced_limit(Some("0")), 1);
        assert_eq!(advanced_limit(Some("-5")), 1);
        assert_eq!(advanced_limit(Some("51")), 50);
        assert_eq!(advanced_limit(Some("99999999999999999999999999")), 50);
        assert_eq!(advanced_limit(Some("-99999999999999999999999999")), 1);
        // Garbage falls back to the default.
        for bad in ["", "abc", "12.5", "0x10"] {
            assert_eq!(advanced_limit(Some(bad)), 10, "input {bad:?}");
        }
    }

    #[test]
    fn state_groups_match_constants_tuple() {
        assert_eq!(
            open_state_groups(),
            &["backlog", "unstarted", "started", "review", "test"]
        );
        assert_eq!(closed_state_groups(), &["completed", "cancelled"]);
        // Slices of the shared tuple, not copies.
        assert_eq!(
            [open_state_groups(), closed_state_groups()].concat(),
            STATE_ORDER
        );
    }

    #[test]
    fn status_vectors() {
        assert_eq!(parse_status(None), StatusFilter::All);
        assert_eq!(parse_status(Some("all")), StatusFilter::All);
        assert_eq!(parse_status(Some("")), StatusFilter::All);
        assert_eq!(parse_status(Some("bogus")), StatusFilter::All);
        assert_eq!(parse_status(Some("OPEN")), StatusFilter::Open);
        assert_eq!(parse_status(Some("Closed")), StatusFilter::Closed);
    }

    #[test]
    fn status_where_shapes() {
        assert_eq!(status_where_sql(StatusFilter::All), None);
        assert_eq!(
            status_where_sql(StatusFilter::Closed).as_deref(),
            Some("\"states\".\"group\" IN ('completed', 'cancelled')")
        );
        assert_eq!(
            status_where_sql(StatusFilter::Open).as_deref(),
            Some("(\"states\".\"group\" IN ('backlog', 'unstarted', 'started', 'review', 'test') OR \"issues\".\"state_id\" IS NULL)")
        );
    }

    #[test]
    fn project_param_vectors() {
        let uuid = "d715be3d-234f-46ef-89a3-97f0c7c04b7e";
        assert_eq!(parse_project_param(None), ProjectRef::None);
        assert_eq!(parse_project_param(Some("")), ProjectRef::None);
        assert_eq!(
            parse_project_param(Some(uuid)),
            ProjectRef::Uuid(Uuid::parse_str(uuid).unwrap())
        );
        assert!(matches!(
            parse_project_param(Some("D715BE3D-234F-46EF-89A3-97F0C7C04B7E")),
            ProjectRef::Uuid(_)
        ));
        assert!(matches!(
            parse_project_param(Some("d715be3d234f46ef89a397f0c7c04b7e")),
            ProjectRef::Uuid(_)
        ));
        assert!(matches!(
            parse_project_param(Some("{d715be3d-234f-46ef-89a3-97f0c7c04b7e}")),
            ProjectRef::Uuid(_)
        ));
        assert!(matches!(
            parse_project_param(Some("urn:uuid:d715be3d-234f-46ef-89a3-97f0c7c04b7e")),
            ProjectRef::Uuid(_)
        ));
        assert_eq!(
            parse_project_param(Some("CT00003")),
            ProjectRef::Identifier("CT00003")
        );
        // Surrounding whitespace fails UUID validation → identifier path.
        assert!(matches!(
            parse_project_param(Some(" {uuid} ")),
            ProjectRef::Identifier(_)
        ));
    }

    #[test]
    fn normalize_identifier_vectors() {
        assert_eq!(normalize_project_identifier("ct00003"), "CT00003");
        assert_eq!(normalize_project_identifier("  eng  "), "ENG");
        assert_eq!(normalize_project_identifier("CT00003"), "CT00003");
    }

    #[test]
    fn resolve_fragments_pinned() {
        assert_eq!(
            resolve_identifier_joins_sql(),
            "INNER JOIN \"workspaces\" ON (\"projects\".\"workspace_id\" = \"workspaces\".\"id\")"
        );
        // The doubled deleted_at arm is Django's no-dedupe rendering.
        assert_eq!(
            resolve_identifier_where_sql()
                .matches("deleted_at\" IS NULL")
                .count(),
            2
        );
        assert_eq!(
            resolve_identifier_tail_sql(),
            "ORDER BY \"projects\".\"created_at\" DESC LIMIT 1"
        );
    }

    // ------------------------------------------------------------------
    // ?since= vectors (probed against Django 4.2.30 + CPython 3.12)
    // ------------------------------------------------------------------

    fn since_iso(raw: &str) -> Option<String> {
        match parse_since(Some(raw)) {
            SinceOutcome::At(at) => Some(at.format("%Y-%m-%dT%H:%M:%S%.6f%:z").to_string()),
            _ => None,
        }
    }

    #[test]
    fn since_absent_or_empty_means_no_filter() {
        assert_eq!(parse_since(None), SinceOutcome::NoFilter);
        assert_eq!(parse_since(Some("")), SinceOutcome::NoFilter);
    }

    #[test]
    fn since_ok_vectors() {
        // (input, expected UTC instant)
        let cases = [
            ("2025-01-01T00:00:00Z", "2025-01-01T00:00:00.000000+00:00"),
            (
                "2025-01-01T00:00:00+00:00",
                "2025-01-01T00:00:00.000000+00:00",
            ),
            ("2025-01-01T00:00:00", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-01-01 00:00:00", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-01-01", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-1-1T0:0", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-01-01T00:00", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-01-01T00", "2025-01-01T00:00:00.000000+00:00"),
            (
                "2025-01-01T00:00:00.123456",
                "2025-01-01T00:00:00.123456+00:00",
            ),
            (
                "2025-01-01T00:00:00,123456",
                "2025-01-01T00:00:00.123456+00:00",
            ),
            (
                "2025-01-01T00:00:00.123456789012",
                "2025-01-01T00:00:00.123456+00:00",
            ),
            (
                "2025-01-01T00:00:00.9999999",
                "2025-01-01T00:00:00.999999+00:00",
            ),
            (
                "2025-01-01T00:00:00+0000",
                "2025-01-01T00:00:00.000000+00:00",
            ),
            ("2025-01-01T00:00:00+00", "2025-01-01T00:00:00.000000+00:00"),
            (
                "2025-01-01T00:00:00+00:00:00",
                "2025-01-01T00:00:00.000000+00:00",
            ),
            (
                "2025-01-01T00:00:00+00:00:30",
                "2024-12-31T23:59:30.000000+00:00",
            ),
            ("20250101T000000", "2025-01-01T00:00:00.000000+00:00"),
            ("20250101", "2025-01-01T00:00:00.000000+00:00"),
            ("20250101T00:00:00", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-W01-1", "2024-12-30T00:00:00.000000+00:00"),
            ("2025W011", "2024-12-30T00:00:00.000000+00:00"),
            ("2025-W01-1T00:00:00", "2024-12-30T00:00:00.000000+00:00"),
            (
                "2025-W01-1T000000+05:30",
                "2024-12-29T18:30:00.000000+00:00",
            ),
            ("2024-02-29T12:30:45.1Z", "2024-02-29T12:30:45.100000+00:00"),
            ("0001-01-01T00:00:00Z", "0001-01-01T00:00:00.000000+00:00"),
            ("9999-12-31T23:59:59Z", "9999-12-31T23:59:59.000000+00:00"),
            ("2025-01-01t00:00:00Z", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-01-01t00:00:00", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-01-01T00:00+00:00", "2025-01-01T00:00:00.000000+00:00"),
            (
                "2025-01-01 00:00:00+00:00",
                "2025-01-01T00:00:00.000000+00:00",
            ),
            ("2025-01-01 00:00+00:00", "2025-01-01T00:00:00.000000+00:00"),
            (
                "2025-01-01T00:00:00.1234567",
                "2025-01-01T00:00:00.123456+00:00",
            ),
            (
                "2025-01-01T00:00:00+05:30",
                "2024-12-31T18:30:00.000000+00:00",
            ),
            (
                "2025-01-01T00:00:00-00:61",
                "2025-01-01T01:01:00.000000+00:00",
            ),
            (
                "2025-01-01T00:00:00+00:61",
                "2024-12-31T22:59:00.000000+00:00",
            ),
            (
                "2025-01-01T00:00:00+00:60",
                "2024-12-31T23:00:00.000000+00:00",
            ),
            (
                "2025-01-01T00:00:00 +00:00",
                "2025-01-01T00:00:00.000000+00:00",
            ),
            (
                "2025-01-01T00:00:00  +00:00",
                "2025-01-01T00:00:00.000000+00:00",
            ),
            ("2025-01-01X00:00:00", "2025-01-01T00:00:00.000000+00:00"),
            (
                "2025-01-01T00:00:00+23:59",
                "2024-12-31T00:01:00.000000+00:00",
            ),
            (
                "2025-01-01T00:00:00+2359",
                "2024-12-31T00:01:00.000000+00:00",
            ),
            (
                "2025-01-01T00:00:00-23:59",
                "2025-01-01T23:59:00.000000+00:00",
            ),
            (
                "2025-01-01T00:00:00+14:00",
                "2024-12-31T10:00:00.000000+00:00",
            ),
            ("2025-01-01T00:00.5", "2025-01-01T00:00:00.500000+00:00"),
            ("2025-01-01T00.5", "2025-01-01T00:00:00.500000+00:00"),
            ("2025-01-01T01.5", "2025-01-01T01:00:00.500000+00:00"),
            ("2025-01-01 00:00:00Z", "2025-01-01T00:00:00.000000+00:00"),
            ("20250101T000000+00:00", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-01-01T000000", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-01-01T00:00:00,5", "2025-01-01T00:00:00.500000+00:00"),
            // Trailing fraction after a *zero* offset is dropped …
            (
                "2025-01-01T00:00:00+00:00,5",
                "2025-01-01T00:00:00.000000+00:00",
            ),
            (
                "2025-01-01T00:00:00+00:00.5",
                "2025-01-01T00:00:00.000000+00:00",
            ),
            (
                "2025-01-01T00:00:00+00:00:00,5",
                "2025-01-01T00:00:00.000000+00:00",
            ),
            (
                "2025-01-01T00:00:00+00:00:00.5",
                "2025-01-01T00:00:00.000000+00:00",
            ),
            // … while after a nonzero offset it extends the offset.
            (
                "2025-01-01T00:00:00+05:30,5",
                "2024-12-31T18:29:59.500000+00:00",
            ),
            (
                "2025-01-01T00:00:00+00:30,5",
                "2024-12-31T23:29:59.500000+00:00",
            ),
            (
                "2025-01-01T00:00:00+01:00,5",
                "2024-12-31T22:59:59.500000+00:00",
            ),
            (
                "2025-01-01T00:00:00+00:00:10,5",
                "2024-12-31T23:59:49.500000+00:00",
            ),
            (
                "2025-01-01T00:00:00+00:00:10.5",
                "2024-12-31T23:59:49.500000+00:00",
            ),
            (
                "2025-01-01T00:00:00-05:30,5",
                "2025-01-01T05:30:00.500000+00:00",
            ),
            (
                "2025-01-01T00:00:00+053000",
                "2024-12-31T18:30:00.000000+00:00",
            ),
            (
                "2025-01-01T00:00:00+053015",
                "2024-12-31T18:29:45.000000+00:00",
            ),
            (
                "2025-01-01T00:00:00+00:00:61",
                "2024-12-31T23:58:59.000000+00:00",
            ),
            ("20250101T000000 +00:00", "2025-01-01T00:00:00.000000+00:00"),
            (
                "2025-W01-1T00:00:00 +00:00",
                "2024-12-30T00:00:00.000000+00:00",
            ),
            // Any single char separates date and time — even `+`/`-`.
            ("2025-01-01+05:30", "2025-01-01T05:30:00.000000+00:00"),
            ("2025-01-01-05:30", "2025-01-01T05:30:00.000000+00:00"),
            ("2025-01-01+00:00", "2025-01-01T00:00:00.000000+00:00"),
            (
                "2025-01-01+000000+00:00",
                "2025-01-01T00:00:00.000000+00:00",
            ),
            ("2025-01-01T00+00:00", "2025-01-01T00:00:00.000000+00:00"),
            ("20250101T00", "2025-01-01T00:00:00.000000+00:00"),
            ("20250101T000000,5", "2025-01-01T00:00:00.500000+00:00"),
            // Python's `$` also matches before one trailing newline.
            ("2025-01-01T00:00:00Z\n", "2025-01-01T00:00:00.000000+00:00"),
            // Trailing space falls through to the regex `\s*` tail.
            ("2025-01-01T00:00:00 ", "2025-01-01T00:00:00.000000+00:00"),
            // Offset-minute overflow normalizes through the tz parser.
            (
                "2025-01-01T00:00:00+00:99",
                "2024-12-31T22:21:00.000000+00:00",
            ),
            // A bare fraction separator with no digits parses (as 0) when
            // a valid zone follows immediately (review fuzz finding).
            ("2025-01-01T00:00:00,Z", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-01-01T00:00:00.Z", "2025-01-01T00:00:00.000000+00:00"),
            (
                "2025-01-01T00:00:00,+05:30",
                "2024-12-31T18:30:00.000000+00:00",
            ),
            ("2025-01-01T00:00,Z", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-01-01T00,Z", "2025-01-01T00:00:00.000000+00:00"),
            ("20250101T000000,Z", "2025-01-01T00:00:00.000000+00:00"),
            // Fraction + space + zone fails `fromisoformat` but the regex
            // layer's `\s*` rescues extended shapes.
            (
                "2025-01-01T00:00:00.5 +00:00",
                "2025-01-01T00:00:00.500000+00:00",
            ),
            // A lone digit immediately before a zone is ignored
            // (value-independent; review fuzz finding).
            ("2025-01-01T234+00", "2025-01-01T23:00:00.000000+00:00"),
            ("2025-01-01T234Z", "2025-01-01T23:00:00.000000+00:00"),
            ("2025-01-01T23456+00", "2025-01-01T23:45:00.000000+00:00"),
            ("2025-01-01T23:456+00", "2025-01-01T23:45:00.000000+00:00"),
            (
                "2025-01-01T23:45:123+00",
                "2025-01-01T23:45:12.000000+00:00",
            ),
            ("2025-01-011234+00", "2025-01-01T23:00:00.000000+00:00"),
            // Single-digit minute/second after a colon fail `fromisoformat`
            // but the regex layer accepts them.
            ("2025-01-01T23:4+00", "2025-01-01T23:04:00.000000+00:00"),
            ("2025-01-01T23:45:6+00", "2025-01-01T23:45:06.000000+00:00"),
            ("2025-01-01T00:00:5+00", "2025-01-01T00:00:05.000000+00:00"),
            // Weekday optional, Monday when absent (review fuzz finding).
            ("2025-W01", "2024-12-30T00:00:00.000000+00:00"),
            ("2025W01", "2024-12-30T00:00:00.000000+00:00"),
            ("2025-W01T00", "2024-12-30T00:00:00.000000+00:00"),
            ("2025W01T00", "2024-12-30T00:00:00.000000+00:00"),
            // After `-`, a digit followed by another digit is time, not a
            // weekday (`-` is the separator).
            ("2025-W01-12", "2024-12-30T12:00:00.000000+00:00"),
            ("2025-W01-12:34", "2024-12-30T12:34:00.000000+00:00"),
            ("2025-W01-12Z", "2024-12-30T12:00:00.000000+00:00"),
            // Basic week: required weekday, then separator-less time.
            ("2025W01112", "2024-12-30T12:00:00.000000+00:00"),
            ("2025W01112:34", "2024-12-30T12:34:00.000000+00:00"),
            // Dash-less week: even a digit is the separator.
            ("2025-W011234+00", "2024-12-30T23:00:00.000000+00:00"),
            ("2025-W01005+00", "2024-12-30T05:00:00.000000+00:00"),
            ("2025-W0112345", "2024-12-30T23:45:00.000000+00:00"),
            // Basic week: invalid weekday digit is skipped, strict time.
            ("2025W01005+00", "2024-12-30T05:00:00.000000+00:00"),
            ("2025W01805+00", "2024-12-30T05:00:00.000000+00:00"),
            ("2025W01905+00", "2024-12-30T05:00:00.000000+00:00"),
            // Basic week: nosep-strict first, then first-digit-as-sep.
            ("2025W01112345+00", "2024-12-30T23:45:00.000000+00:00"),
            ("2025W011123+00", "2024-12-30T23:00:00.000000+00:00"),
            ("2025W01192345+00", "2024-12-30T23:45:00.000000+00:00"),
            ("2025W0112345+00", "2024-12-30T23:45:00.000000+00:00"),
            ("2025W01112x+00", "2024-12-30T12:00:00.000000+00:00"),
            // Drop allowed once a separator was consumed.
            ("2025W01T234+00", "2024-12-30T23:00:00.000000+00:00"),
            ("2025-W01-1T234+00", "2024-12-30T23:00:00.000000+00:00"),
            ("2025W011T2345+00", "2024-12-30T23:45:00.000000+00:00"),
            // A consumed component colon also enables the drop.
            ("2025W01100:005+00", "2024-12-30T00:00:00.000000+00:00"),
            ("2025W01100:00:005+00", "2024-12-30T00:00:00.000000+00:00"),
            ("2025W01112:305+00", "2024-12-30T12:30:00.000000+00:00"),
            ("2025W01123:59:595+00", "2024-12-30T23:59:59.000000+00:00"),
            ("2025-01-01T00:005+00", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-01-01T00:005Z", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-01-01T12:345+00", "2025-01-01T12:34:00.000000+00:00"),
            // Without a separator, bare-digit fractions need an even run.
            ("2025W01123451212+00", "2024-12-30T23:45:12.120000+00:00"),
            // One ASCII junk char (not `Z`/`+`/`-`) before a zone.
            (
                "2025-01-01T12:30:45x+00",
                "2025-01-01T12:30:45.000000+00:00",
            ),
            (
                "2025-01-01T12:30:45:+00",
                "2025-01-01T12:30:45.000000+00:00",
            ),
            ("2025-01-01T12z+00", "2025-01-01T12:00:00.000000+00:00"),
            ("2025-01-01T12\x0b+00", "2025-01-01T12:00:00.000000+00:00"),
            // Six or more fraction digits: scan to the first zone leader
            // (review fuzz finding).
            (
                "2024-02-29T12:30:45.123456:-05:30",
                "2024-02-29T18:00:45.123456+00:00",
            ),
            (
                "2025-01-01T12:30:45.123456:+00",
                "2025-01-01T12:30:45.123456+00:00",
            ),
            (
                "2025-01-01T12:30:45.123456xyz+00",
                "2025-01-01T12:30:45.123456+00:00",
            ),
            (
                "2025-01-01T12:30:45.123456  +00",
                "2025-01-01T12:30:45.123456+00:00",
            ),
            (
                "2025-01-01T12:30:45.123456:12-05:30",
                "2025-01-01T18:00:45.123456+00:00",
            ),
            (
                "2025-01-01T12:30:45.1234567:+00",
                "2025-01-01T12:30:45.123456+00:00",
            ),
            // A `:` not followed by 2 digits is not a component separator
            // (review fuzz finding).
            ("2025-01-01T00:+00", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-01-01T00:00:+00", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-01-01T12:30:+00", "2025-01-01T12:30:00.000000+00:00"),
            ("2025-01-01T12:30:Z", "2025-01-01T12:30:00.000000+00:00"),
            // `:` is a fraction separator after an extended SS.
            (
                "2025-01-01T00:00:00:12+00",
                "2025-01-01T00:00:00.120000+00:00",
            ),
            (
                "2025-01-01T00:00:00:5+00",
                "2025-01-01T00:00:00.500000+00:00",
            ),
            // Bare digits after a basic SS are a fraction (2+ of them; a
            // single digit is skipped instead).
            ("2025-01-01T00000012+00", "2025-01-01T00:00:00.120000+00:00"),
            ("2025-01-01T00000001+00", "2025-01-01T00:00:00.010000+00:00"),
            (
                "2025-01-01T000000001+00",
                "2025-01-01T00:00:00.001000+00:00",
            ),
            ("2025-01-01T0000001+00", "2025-01-01T00:00:00.000000+00:00"),
            ("2025-01-01T12345+00", "2025-01-01T12:34:00.000000+00:00"),
            ("2025-01-01T000012+00", "2025-01-01T00:00:12.000000+00:00"),
            // Single-digit components fail `fromisoformat` but the regex
            // layer accepts them.
            ("2025-01-01T00:5+00", "2025-01-01T00:05:00.000000+00:00"),
            ("2025-01-01T00:1+00", "2025-01-01T00:01:00.000000+00:00"),
            ("2025-01-01T00:00:1+00", "2025-01-01T00:00:01.000000+00:00"),
            ("2025-01-01T00:00:5+00", "2025-01-01T00:00:05.000000+00:00"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                since_iso(input).as_deref(),
                Some(expected),
                "input {input:?}"
            );
        }
    }

    #[test]
    fn since_bad_format_vectors() {
        let cases = [
            "2025-01-01t00:00:00z",
            "2025-01-01T00:00:00z",
            "2025-001",
            "2025001",
            "2025-01",
            "bogus",
            "2025-W54-1",
            "2025-W01-8",
            "T00:00:00",
            "00:00:00",
            "2025-01-01T00:00:00.",
            "2025-01-01T00:00:00,",
            " 2025-01-01T00:00:00Z",
            "2025-01-01T00:00:00Z ",
            "2025-01-01  00:00:00",
            "2025-01-0100:00:00",
            "20250101T000000  +00:00",
            "2025-01-01+25:00",
            "2025-01-01Z",
            "2025-01-01T00:00:00+0",
            "2025-01-01T00:00:00+00:00,5,6",
            "2025-01-01T00:00:00Z,5",
            "2025-01-01T00:00:00+05:30junk",
            "20250101000000",
            // A bad offset on a basic date never reaches the regex (400,
            // not 500 — the layering prediction, probed).
            "20250101T000000+25:00",
            // Invalid *date-only* values never reach the regex either.
            "2025-13-01",
            "2025-02-30",
            "10000-01-01T00:00:00",
            "2025-01-01T00:00:00+05:3000",
            "2025-01-01T00:00:00Z\n\n",
            // Bare separator needs an immediate zone (review fuzz finding).
            "2025-01-01T00:00:00, Z",
            "2025-01-01T00:00:00 ,Z",
            "2025-01-01T00:00:00,x",
            "2025-01-01T00:00:00,,Z",
            "2025-01-01T00:00:00,5,Z",
            // Fraction + space + zone on a basic date: `fromisoformat`
            // rejects the space and the regex cannot match the shape.
            "20250101T000000.5 +00:00",
            // The lone-digit drop needs an abutting zone (review fuzz).
            "2025-01-01T234",
            "2025-01-01T234.5+00",
            "2025-01-01T234 +00",
            "2025-01-01T23 4+00",
            "2025-01-01T23:4512+00",
            "2025-01-01T2345:6+00",
            "2025-01-01T23:4567+00",
            // Week shapes without a weekday (review fuzz).
            "2025-W54",
            "2025-W0112",
            "2025-W011",
            "2025-W01-112",
            "2025-W01-71",
            "2025-W01-0",
            "2025W010",
            "2025W0112",
            // Basic week: no digit drop without a separator (review fuzz).
            "2025W011234+00",
            "2025W01123456+00",
            "2025W0111234567+00",
            "2025W010234+00",
            "2025W0119234+00",
            "2025W011934+00",
            // ... and odd bare-digit runs are not fractions either.
            "2025W011123456785+00",
            "2025-01-01T12:3456+00",
            // The junk-char skip excludes zone leaders, doublings, gaps,
            // fractions, and non-ASCII (review fuzz).
            "2025-01-01T12++00",
            "2025-01-01T12ZZ",
            "2025-01-01T12:30:45xy+00",
            "2025-01-01T12:30:45x +00",
            "2025-01-01T12:30:45.5x+00",
            "2025-01-01T12é+00",
            // The 6-digit scan needs a valid zone at the first leader;
            // short fractions stay strict (review fuzz).
            "2025-01-01T12:30:45.123456xyz",
            "2025-01-01T12:30:45.123456a-b+00",
            "2025-01-01T12:30:45.123456+-05:30",
            "2025-01-01T12:30:45.123456Z+00",
            "2025-01-01T12:30:45.123456+00junk",
            "2025-01-01T12:30:45.5:+00",
            // No second fraction, and `:` after basic components is not a
            // fraction separator (review fuzz).
            "2025-01-01T00:00:00.5:12+00",
            "2025-01-01T00:00:00:12:34+00",
            "2025-01-01T00::12+00",
            "2025-01-01T0000:12+00",
            "2025-01-01T000000:12+00",
            "2025-01-01T00:00:0012+00",
        ];
        for input in cases {
            match parse_since(Some(input)) {
                SinceOutcome::BadFormat(err) => {
                    assert_eq!(err.key, "error");
                    assert_eq!(
                        err.body(),
                        "{\"error\":\"Invalid 'since' — expected ISO 8601 datetime (e.g. 2025-01-01T00:00:00Z).\"}",
                        "input {input:?}"
                    );
                }
                other => panic!("input {input:?} parsed as {other:?}, want 400"),
            }
        }
        // The 400 body is exactly the fixture pin.
        let fx = fixture();
        assert_eq!(
            unit(&fx, "advanced_search_live")["bad_since"]["body"]["error"]
                .as_str()
                .unwrap(),
            SINCE_ERROR_MESSAGE
        );
    }

    #[test]
    fn since_invalid_value_vectors() {
        // Well-formed but invalid → ValueError propagates → 500.
        for input in [
            "2025-13-01T00:00:00",
            "2025-01-32T00:00:00",
            "2025-01-01T25:00:00",
            "2025-01-01T24:00:00",
            "2025-01-01T00:00:60",
            "2025-01-01T00:00:00+25:00",
            "2025-01-01T00:00:00+24:00",
            "2025-01-01T00:00:00+99:99",
            "2025-02-30T00:00:00",
            "0000-01-01T00:00:00",
        ] {
            assert_eq!(
                parse_since(Some(input)),
                SinceOutcome::InvalidValue(SearchBug500::BadSinceValue),
                "input {input:?}"
            );
        }
    }

    // ------------------------------------------------------------------
    // Advanced SQL + assembly replay
    // ------------------------------------------------------------------

    #[test]
    fn advanced_values_keys_match_source() {
        assert_eq!(ADVANCED_VALUES_KEYS.len(), 14);
        let fx = fixture();
        let row = &unit(&fx, "advanced_search_queryset")["rows"][0];
        for key in ADVANCED_VALUES_KEYS {
            assert!(row.get(key).is_some(), "row lacks {key}");
        }
    }

    #[test]
    fn advanced_select_carries_all_fourteen() {
        let select = advanced_select_sql();
        // The rank/headline expressions embed commas, so count the plain
        // columns instead of splitting.
        for column in [
            "\"issues\".\"id\"",
            "\"issues\".\"sequence_id\"",
            "\"issues\".\"name\"",
            "\"issues\".\"created_at\"",
            "\"issues\".\"updated_at\"",
            "\"issues\".\"completed_at\"",
            "\"states\".\"name\"",
            "\"states\".\"group\"",
            "\"issues\".\"project_id\"",
            "\"projects\".\"identifier\"",
            "\"projects\".\"name\"",
            "\"workspaces\".\"slug\"",
        ] {
            assert!(select.contains(column), "select lacks {column}");
        }
        // Quirk 13: the recorded SQL omits updated_at/completed_at; the
        // source (and the recorded rows) select them.
        assert!(select.contains("\"issues\".\"updated_at\""));
        assert!(select.contains("\"issues\".\"completed_at\""));
        assert!(select.contains("AS \"_headline\""));
        assert!(select.contains("AS \"_rank\""));
    }

    #[test]
    fn advanced_sql_replays_fixture() {
        let fx = fixture();
        let recorded = unit(&fx, "advanced_search_queryset")["sql"]
            .as_str()
            .expect("sql recorded");
        let parts = AdvancedParts {
            query: "zxcvsearchtoken",
            project_uuid_filter: false,
            status: StatusFilter::All,
            since_filter: false,
            sort: Sort::Rank,
        };
        let built = advanced_search_sql(&parts);
        // Every recorded plain column is selected here (mine adds the two
        // the shell reconstruction dropped — quirk 13). The FTS
        // expressions split apart on ", ", so only table-prefixed items
        // compare; rank/headline assert separately below.
        let recorded_items: Vec<String> = norm(recorded.split(" FROM ").next().unwrap())
            .trim_start_matches("select distinct ")
            .split(", ")
            .map(str::to_owned)
            .collect();
        let built_select = norm(built.split(" FROM ").next().unwrap());
        for item in &recorded_items {
            if !item.starts_with("issues.")
                && !item.starts_with("states.")
                && !item.starts_with("projects.")
                && !item.starts_with("workspaces.")
            {
                continue;
            }
            assert!(built_select.contains(item), "select lacks {item}");
        }
        assert!(built_select.contains("ts_rank"));
        assert!(built_select.contains("ts_headline"));
        // Comment arm present (include_comments=True).
        assert!(built.contains("issue_comments"));
        // Tail: alias ordering where Django emits the positional `10`.
        let recorded_tail = norm(recorded.rsplit("ORDER BY").next().unwrap())
            .replace("10 desc", "_rank desc")
            .replace("limit 10", "limit :limit");
        let built_tail = norm(built.rsplit("ORDER BY").next().unwrap());
        assert_eq!(built_tail, recorded_tail);
    }

    #[test]
    fn advanced_sql_full_filters_in_source_order() {
        let parts = AdvancedParts {
            query: "x",
            project_uuid_filter: true,
            status: StatusFilter::Open,
            since_filter: true,
            sort: Sort::Updated,
        };
        let built = advanced_search_sql(&parts);
        let project = built.find(PROJECT_UUID_WHERE).expect("project arm");
        let status = built.find("states\".\"group\" IN").expect("status arm");
        let since = built.find(SINCE_WHERE).expect("since arm");
        let fts = built.find("@@ websearch_to_tsquery").expect("fts arm");
        assert!(
            project < status && status < since && since < fts,
            "filter order: {built}"
        );
        assert!(built.ends_with("ORDER BY \"issues\".\"updated_at\" DESC LIMIT :limit"));
    }

    #[test]
    fn advanced_result_replays_fixture_first() {
        let fx = fixture();
        let row = &unit(&fx, "advanced_search_queryset")["rows"][0];
        let first = &unit(&fx, "advanced_search_live")["first"];
        let get = |key: &str| row[key].as_str().expect("row str");
        let built = advanced_result(
            &AdvancedRow {
                id: get("id"),
                sequence_id: get("sequence_id").parse().expect("int4"),
                name: get("name"),
                headline: Some(get("_headline")),
                rank: Some(get("_rank").parse().expect("ts_rank float")),
                // Datetimes arrive DRF-rendered (the live forms).
                created_at: first["created_at"].as_str().unwrap(),
                updated_at: first["updated_at"].as_str().unwrap(),
                completed_at: None,
                state_name: Some(get("state__name")),
                state_group: Some(get("state__group")),
                project_id: get("project_id"),
                project_identifier: get("project__identifier"),
                project_name: get("project__name"),
                workspace_slug: get("workspace__slug"),
            },
            first["url"].as_str(),
        );
        // The recorded `first` carries two `str()` artifacts (shell
        // reconstruction): `sequence_id`/`rank` as strings and
        // `completed_at` as `"None"`. The live wire has number/null.
        let mut expected = first.clone();
        expected["sequence_id"] = serde_json::json!(1);
        expected["completed_at"] = Value::Null;
        expected["rank"] = serde_json::json!(0.075990885);
        assert_eq!(built, expected);
        // Key order is source order, independent of value comparison.
        let keys: Vec<&str> = built
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "id",
                "sequence_id",
                "identifier",
                "name",
                "snippet",
                "state",
                "project",
                "workspace_slug",
                "created_at",
                "updated_at",
                "completed_at",
                "rank",
                "url"
            ]
        );
    }

    #[test]
    fn advanced_result_stateless_nulls_and_missing_url() {
        let built = advanced_result(
            &AdvancedRow {
                id: "id",
                sequence_id: 7,
                name: "n",
                headline: None,
                rank: None,
                created_at: "c",
                updated_at: "u",
                completed_at: None,
                state_name: None,
                state_group: None,
                project_id: "p",
                project_identifier: "PR",
                project_name: "pn",
                workspace_slug: "w",
            },
            None,
        );
        assert_eq!(built["snippet"], Value::String(String::new()));
        assert_eq!(built["state"]["name"], Value::Null);
        assert_eq!(built["state"]["group"], Value::Null);
        assert_eq!(built["completed_at"], Value::Null);
        assert_eq!(built["rank"], serde_json::json!(0.0));
        assert!(built.get("url").is_none());
        assert_eq!(built["identifier"], Value::String("PR-7".to_owned()));
    }

    #[test]
    fn advanced_envelope_shape() {
        let envelope = advanced_envelope("q", vec![Value::Null, Value::Null]);
        assert_eq!(envelope["query"], Value::String("q".to_owned()));
        assert_eq!(envelope["count"], serde_json::json!(2));
        assert_eq!(envelope["results"].as_array().unwrap().len(), 2);
        let keys: Vec<&str> = envelope
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["query", "count", "results"]);
    }

    // ------------------------------------------------------------------
    // Page reads
    // ------------------------------------------------------------------

    #[test]
    fn page_joins_and_order_match_fixture() {
        let fx = fixture();
        let sql = unit(&fx, "page_get_queryset")["sql"].as_str().unwrap();
        assert!(norm(sql).contains(&norm(&page_joins_sql())));
        assert!(norm(sql).ends_with(&norm(&format!("ORDER BY {PAGE_META_ORDER}"))));
    }

    #[test]
    fn page_visibility_where_full_replay() {
        let fx = fixture();
        let recorded = unit(&fx, "page_get_queryset")["sql"]
            .as_str()
            .expect("sql recorded");
        // Extract the recorded WHERE (after the first WHERE, before
        // the last ORDER BY — the EXISTS subquery nests a second WHERE).
        let recorded_where = norm(
            recorded
                .split_once(" WHERE ")
                .unwrap()
                .1
                .rsplit_once(" ORDER BY ")
                .unwrap()
                .0,
        );
        // Map placeholders to the recorded live values.
        let built_where = norm(&page_visibility_where())
            .replace(":slug", "ws-conv659-2")
            .replace(":member_id", "79c81d76-5a93-4d3d-894d-5935576834b6")
            .replace(":project_id", "d715be3d-234f-46ef-89a3-97f0c7c04b7e");
        // The fixture wraps the whole chain in one paren pair.
        let built_where = format!("({built_where})");
        assert_eq!(built_where, recorded_where);
    }

    #[test]
    fn validate_parent_vectors() {
        let page = Uuid::parse_str("255f3646-9ace-4f46-9620-788071f7a05e").unwrap();
        let other = Uuid::parse_str("a7509d00-345f-47fb-bee3-6bcf7d3339e2").unwrap();
        let third = Uuid::parse_str("d715be3d-234f-46ef-89a3-97f0c7c04b7e").unwrap();
        let no_fetch = &|_: Uuid| -> Option<Uuid> { None };
        // Absent parent is always fine.
        assert_eq!(
            validate_parent(ParentLookup::Absent, Some(page), no_fetch),
            None
        );
        assert_eq!(validate_parent(ParentLookup::Absent, None, no_fetch), None);
        // Missing row 400s.
        assert_eq!(
            validate_parent(ParentLookup::Missing, Some(page), no_fetch),
            Some(ValidateParentError::NotFound)
        );
        // Archived parent 400s before any walk.
        let archived = ParentLookup::Found(ParentRef {
            id: other,
            archived: true,
        });
        assert_eq!(
            validate_parent(archived, Some(page), no_fetch),
            Some(ValidateParentError::Archived)
        );
        // Create path (no page yet) skips the walk.
        let found = ParentLookup::Found(ParentRef {
            id: other,
            archived: false,
        });
        assert_eq!(validate_parent(found, None, no_fetch), None);
        // Direct self-parent.
        let me = ParentLookup::Found(ParentRef {
            id: page,
            archived: false,
        });
        assert_eq!(
            validate_parent(me, Some(page), no_fetch),
            Some(ValidateParentError::SelfNest)
        );
        // Descendant-as-parent: other → page.
        let fetch = &move |id: Uuid| -> Option<Uuid> {
            if id == other {
                Some(page)
            } else {
                None
            }
        };
        assert_eq!(
            validate_parent(found, Some(page), fetch),
            Some(ValidateParentError::SelfNest)
        );
        // Clean chain: other → third → top.
        let fetch = &move |id: Uuid| -> Option<Uuid> {
            if id == other {
                Some(third)
            } else {
                None
            }
        };
        assert_eq!(validate_parent(found, Some(page), fetch), None);
        // A top-level parent ends the walk at once (its parent id is
        // NULL, which reads exactly like a missing row — quirk 11).
        assert_eq!(validate_parent(found, Some(page), no_fetch), None);
        // A stored cycle cannot hang the walk.
        let fetch = &move |id: Uuid| -> Option<Uuid> {
            if id == other {
                Some(third)
            } else if id == third {
                Some(other)
            } else {
                None
            }
        };
        assert_eq!(validate_parent(found, Some(page), fetch), None);
    }

    #[test]
    fn validate_parent_bodies_match_fixture() {
        let fx = fixture();
        let validate = unit(&fx, "validate_parent");
        assert!(validate["none_ok"].as_bool().unwrap());
        assert_eq!(
            serde_json::from_str::<Value>(&ValidateParentError::NotFound.body()).unwrap(),
            validate["missing"]["body"]
        );
        assert_eq!(
            serde_json::from_str::<Value>(&ValidateParentError::SelfNest.body()).unwrap(),
            validate["self_nest"]["body"]
        );
        assert_eq!(
            ValidateParentError::Archived.body(),
            "{\"error\":\"Parent page is archived\"}"
        );
    }

    #[test]
    fn page_not_found_matches_fixture() {
        assert_eq!(PAGE_NOT_FOUND_STATUS, 404);
        assert_eq!(page_not_found_body(), "{\"error\":\"Page not found\"}");
        let fx = fixture();
        let missing = &unit(&fx, "get_page_or_error")["missing"];
        assert_eq!(missing["status"], serde_json::json!(404));
        assert_eq!(
            serde_json::from_str::<Value>(&page_not_found_body()).unwrap(),
            missing["body"]
        );
    }

    #[test]
    fn fetch_first_vs_detail_get_shapes() {
        let first = page_fetch_first_sql("*");
        assert!(first.contains(&format!("ORDER BY {PAGE_META_ORDER} LIMIT 1")));
        assert!(first.contains("\"pages\".\"id\" = :page_id"));
        let get = page_detail_get_sql("*");
        assert!(get.ends_with("LIMIT 21"));
        assert!(!get.contains("ORDER BY"));
        assert!(get.contains("\"pages\".\"id\" = :page_id"));
        // The minimal parent lookup narrows the projection, same row.
        let parent = parent_fetch_sql();
        assert!(parent.starts_with("SELECT \"pages\".\"id\", \"pages\".\"archived_at\" FROM"));
        assert!(parent.contains("LIMIT 1"));
    }

    #[test]
    fn parent_lookup_shape() {
        assert_eq!(
            parent_lookup_sql(),
            "SELECT \"pages\".\"parent_id\" FROM \"pages\" WHERE (\"pages\".\"deleted_at\" IS NULL AND \"pages\".\"id\" = :page_id) ORDER BY \"pages\".\"created_at\" DESC LIMIT 1"
        );
    }

    #[test]
    fn archive_admin_exists_shape() {
        assert_eq!(PROJECT_ROLE_ADMIN, 20);
        let sql = archive_admin_exists_sql();
        assert!(sql.starts_with("SELECT (1) AS \"a\" FROM \"project_members\" WHERE ("));
        assert!(sql.ends_with(") LIMIT 1"));
        assert!(!sql.contains("ORDER BY"));
        for arm in [
            "\"project_members\".\"deleted_at\" IS NULL",
            "\"project_members\".\"project_id\" = :project_id",
            "\"project_members\".\"member_id\" = :member_id",
            "\"project_members\".\"is_active\"",
            "\"project_members\".\"role\" = 20",
        ] {
            assert!(sql.contains(arm), "admin probe lacks {arm}");
        }
    }

    // ------------------------------------------------------------------
    // FTS parity pins
    // ------------------------------------------------------------------

    #[test]
    fn fts_parity_pins_hold() {
        let fx = fixture();
        let pins = unit(&fx, "fts_parity_pins");
        assert_eq!(pins["config"].as_str().unwrap(), "english");
        assert_eq!(pins["config"].as_str().unwrap(), fts::FTS_CONFIG);
        // The built chain embeds the shared vectors (index-expression
        // parity is owned by fts.rs tests + FX-FTS-CORE).
        let chain = search_fts_where("x", true).expect("applies");
        assert!(chain.contains(&fts::issue_vector_sql("issues")));
        assert!(chain.contains(&fts::comment_vector_sql("issue_comments")));
        assert!(chain.contains(&fts::websearch_sql(":fts")));
        // Snippet pins: filler and NULL both read as "".
        assert_eq!(pins["snippet_no_match"].as_str().unwrap(), "");
        assert_eq!(pins["snippet_null"].as_str().unwrap(), "");
        assert_eq!(fts::extract_snippet(None), "");
        assert_eq!(fts::extract_snippet(Some("plain filler text")), "");
        assert_eq!(
            fts::extract_snippet(Some("the <<login>> flow breaks")),
            "the login flow breaks"
        );
        // Rank/headline shapes flow through the select list.
        let select = advanced_select_sql();
        assert!(select.contains(&fts::rank_sql("issues", ":fts")));
        assert!(select.contains(&fts::headline_sql(":fts")));
    }
}

#[cfg(test)]
mod pidashconv_736_tests {
    use super::normalize_project_identifier;

    #[test]
    fn project_identifier_strips_py_whitespace() {
        assert_eq!(normalize_project_identifier("  eng "), "ENG");
        // Python `str.strip()` also strips U+001C-U+001F (PIDASHCONV-736).
        for sep in ['\u{1c}', '\u{1d}', '\u{1e}', '\u{1f}'] {
            let padded = format!("{sep}eng{sep}");
            assert_eq!(
                normalize_project_identifier(&padded),
                "ENG",
                "U+{:04X} padding must strip like Python",
                sep as u32
            );
        }
        // TAB and U+0085 padding already matched Django; pin the behavior.
        assert_eq!(normalize_project_identifier("\teng\t"), "ENG");
        assert_eq!(normalize_project_identifier("\u{85}eng\u{85}"), "ENG");
    }
}
