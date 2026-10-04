#![forbid(unsafe_code)]

//! Work-item list/detail query builders (D-18 queries A, PIDASHCONV-668).
//!
//! Ports the read querysets behind the v1 work-item list/detail surface to
//! SQL text plus predicate compilers, following the D-25 precedent
//! (`app_project/queries.rs`): each builder returns a fragment the handler
//! splices into the statement it executes. Placeholders stay symbolic —
//! `:slug`, `:project_id`, `:project_identifier`, `:states`,
//! `:state_groups`, `:priorities`, `:parents`, `:label_ids`,
//! `:assignee_ids`, `:external_id`, `:external_source`, `:pk`,
//! `:sequence`, `:project_id_lookup` — handlers bind them.
//!
//! Sources (drift baseline `01a93e17`; all paths under `apps/api/pi_dash/`):
//! - `api/views/issue.py:206-223` — `WorkspaceIssueAPIEndpoint.get_queryset`
//! - `api/views/issue.py:250-260` — workspace `get` inline `.get()` lookup
//! - `api/views/issue.py:283-300` — `IssueListCreateAPIEndpoint.get_queryset`
//! - `api/views/issue.py:344-359` — external-id early return lookup
//! - `api/views/issue.py:361-437` — list ordering (4 branches)
//! - `api/views/issue.py:368-370` — `work_item_list_filters` call site
//! - `api/views/issue.py:439-444` — `paginate` call (`total_count_queryset`)
//! - `api/views/issue.py:552-569` — `IssueDetailAPIEndpoint.get_queryset`
//! - `api/views/issue.py:600-605` — detail `get` inline `.get()` lookup
//! - `api/views/issue.py:639-767` — `put` upsert read path
//! - `api/views/issue.py:796` — `patch` lookup
//! - `api/views/issue.py:884-897` — `delete` lookup (+ admin/creator check)
//! - `utils/issue_filters.py:485-654` — `IssueFilterError`,
//!   `work_item_list_filters` and its `_resolve_*` helpers
//! - `utils/paginator.py:635-694` — `BasePaginator.get_per_page`/`paginate`
//!   call surface (cursor math itself is the shared `pidash_api::paginator`
//!   kernel, reused by handlers, never forked here)
//! - `db/models/issue.py:95-103` — `IssueManager` scope
//! - `db/mixins.py:56-68` — `SoftDeletionManager` (`objects` scope)
//! - `db/models/state.py:79-84,129` — `StateManager` + `sequence` ordering
//! - `db/models/label.py:27-45` — label table + `-created_at` ordering
//! - `utils/constants.py:76` — `STATE_GROUP_ORDER`
//!
//! Fixture oracle: F18-06
//! (`rust-api/fixtures/v1_work_items/queries/F18-06.list_detail.json` +
//! `TRACE.md`). The unit tests below pin the builders against that file so
//! transcription drift fails the build. Fixture SQL carries live values
//! (unquoted `str(query)` rendering); builders emit symbolic placeholders,
//! so replay tests normalize both sides (quotes/whitespace/case-sensitive
//! literals) before comparing.
//!
//! Reuse (never fork):
//! - `crate::app_issues::params::{parse_per_page, ParamError}` — identical
//!   `per_page`/error-body semantics, tested here against the F18-06 pins.
//! - `crate::app_issues::ordering::{STATE_ORDER, PRIORITY_ORDER}` — the same
//!   `STATE_GROUP_ORDER` tuple and priority list.
//! - `pidash_db::issue_filters::issue_filters_get` — the legacy GET leaves
//!   (`state__in`, `state__group__in`, `priority__in`, `parent__isnull`,
//!   `parent__in`) this compiler normalizes into.
//! - `pidash_api::paginator` — cursor/offset math for handlers.
//!
//! Ported bugs and quirks (translation, don't redesign; also listed in the
//! PR):
//! 1. `order_by=state__name` orders by state *group*, not name: the state
//!    branch tests `state__name` too and builds the group `CASE`.
//! 2. `cycle_id` carries `deleted_at IS NULL` twice (manager scope plus the
//!    explicit `deleted_at__isnull=True`; Django does not dedupe).
//! 3. `Count` keeps its capital C (`Func(F("id"), function="Count")`).
//! 4. `total_count_queryset` skips `.distinct()` when no filters are present
//!    (the `if filters:` guard) but applies it otherwise.
//! 5. The workspace/detail `get_queryset`s are dead on the wire: both `get`
//!    handlers use inline `.get()` chains (no `select_related`, no
//!    `distinct`). Both shapes are ported, never unified.
//! 6. `get_queryset` reads `order_by` from URL *kwargs* (never present — it
//!    is a query param), so the default `-created_at` always applies; the
//!    list `GET` then re-orders, replacing it outright.
//! 7. Priority/state ordering has no tiebreak (unlike the app ordering,
//!    which appends `-created_at`): a single `ORDER BY` key replaces the
//!    queryset order.
//! 8. `labels__name`/`assignees__first_name` order by `MAX` (not `MIN`),
//!    alias `max_values`, with the aggregate inlined into `ORDER BY` and a
//!    `GROUP BY` over the select list.
//! 9. The label "valid" list falls back to `(none)` when the scope is empty;
//!    the state list has no fallback (renders `Valid states: .`).
//! 10. The valid-states/labels sort is by lowercase with an arbitrary
//!     tiebreak in Python (hash-ordered set); the port breaks ties by the
//!     original string for determinism.
//! 11. UUID filter tokens pass through verbatim (braces/case preserved) into
//!     the legacy leaves, which re-parse them; unparseable tokens vanish
//!     silently there — unreachable from this compiler, which validates.
//! 12. `order_by` on an annotation (`cycle_id`, `link_count`, …) orders by
//!     the annotation; multi-level `__` paths needing new joins are
//!     unsupported (Django fans out; the port 500s — documented in
//!     [`OrderSpec::Unsupported`]).
//!
//! Out of scope (sibling issues): serializer shaping (660-666), subresource
//! / search / page reads (669, 670), guards (671), task enqueues (672),
//! handler assembly + routes (673+), column lists (667).

use std::collections::HashMap;

use chrono::NaiveDate;

use crate::app_issues::ordering::{PRIORITY_ORDER, STATE_ORDER};
use crate::app_issues::params::ParamError;
use pidash_db::issue_filters::{issue_filters_get, FilterValue, IssueFilter};

// ---------------------------------------------------------------------------
// Base scope: IssueManager + select_related joins + sub_issues_count
// ---------------------------------------------------------------------------

/// `IssueManager.get_queryset` (`db/models/issue.py:95-103`) over the base
/// `"issues"` alias, in fixture predicate order: live rows, non-triage
/// (with Django's `NULL` guard so stateless issues are kept), unarchived,
/// unarchived project, non-draft.
pub const ISSUE_MANAGER_WHERE: &str = "\"issues\".\"deleted_at\" IS NULL AND NOT (\"states\".\"group\" = 'triage' AND \"states\".\"group\" IS NOT NULL) AND NOT (\"issues\".\"archived_at\" IS NOT NULL) AND NOT (\"projects\".\"archived_at\" IS NOT NULL) AND NOT (\"issues\".\"is_draft\")";

/// `select_related("project", "workspace", "state", "parent")` join shape
/// (`views/issue.py:216-219,293-296,561-564`), in fixture order. Nullable
/// FKs (`state`, `parent`) render `LEFT OUTER JOIN`; non-null FKs render
/// `INNER JOIN`. The parent self-join alias is `parent_issue` (Django's
/// `T5` is a join-order artifact).
pub fn base_joins_sql() -> String {
    [
        "LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\")",
        "INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\")",
        "INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\")",
        "LEFT OUTER JOIN \"issues\" parent_issue ON (\"issues\".\"parent_id\" = parent_issue.\"id\")",
    ]
    .join(" ")
}

/// The `select_related` chain in source order (for handlers composing the
/// select list).
pub const SELECT_RELATED: &[&str] = &["project", "workspace", "state", "parent"];

/// The `prefetch_related` chains (`:220-221,297-298,565-566`): separate
/// handler-owned queries, no list-SQL effect.
pub const PREFETCH_RELATED: &[&str] = &["assignees", "labels"];

/// `sub_issues_count` annotation (`:207-212,284-289,553-558`): a `Count`
/// (capital C, ported quirk 3) of live child issues under the manager
/// scope, correlated on the outer row. The inner `.order_by()` clears the
/// `-created_at` default, so the subquery carries no `ORDER BY`.
pub fn sub_issues_count_sql() -> String {
    "(SELECT Count(sub_issue.\"id\") AS \"count\" FROM \"issues\" sub_issue LEFT OUTER JOIN \"states\" sub_state ON (sub_issue.\"state_id\" = sub_state.\"id\") INNER JOIN \"projects\" sub_project ON (sub_issue.\"project_id\" = sub_project.\"id\") WHERE (sub_issue.\"deleted_at\" IS NULL AND NOT (sub_state.\"group\" = 'triage' AND sub_state.\"group\" IS NOT NULL) AND NOT (sub_issue.\"archived_at\" IS NOT NULL) AND NOT (sub_project.\"archived_at\" IS NOT NULL) AND NOT (sub_issue.\"is_draft\") AND sub_issue.\"parent_id\" = (\"issues\".\"id\"))) AS \"sub_issues_count\"".to_owned()
}

/// Workspace scoping of `WorkspaceIssueAPIEndpoint.get_queryset`
/// (`:213-214`), in fixture predicate order.
pub fn workspace_scope_sql() -> String {
    "\"workspaces\".\"slug\" = :slug AND \"projects\".\"identifier\" = :project_identifier"
        .to_owned()
}

/// Project scoping of the list/detail `get_queryset`s (`:290-291,559-560`),
/// in fixture predicate order.
pub fn project_scope_sql() -> String {
    "\"issues\".\"project_id\" = :project_id AND \"workspaces\".\"slug\" = :slug".to_owned()
}

/// Full `WHERE` of `WorkspaceIssueAPIEndpoint.get_queryset` (`:206-223`).
pub fn workspace_queryset_where() -> String {
    format!("{} AND {}", ISSUE_MANAGER_WHERE, workspace_scope_sql())
}

/// Full `WHERE` of `IssueListCreateAPIEndpoint.get_queryset` (`:283-300`).
pub fn list_queryset_where() -> String {
    format!("{} AND {}", ISSUE_MANAGER_WHERE, project_scope_sql())
}

/// Full `WHERE` of `IssueDetailAPIEndpoint.get_queryset` (`:552-569`) —
/// the identical chain to [`list_queryset_where`] (ported quirk 5: dead on
/// the wire, ported because the issue names it).
pub fn detail_queryset_where() -> String {
    list_queryset_where()
}

/// Every `get_queryset` above ends in `.distinct()` (`:222,299,568`).
pub const QUERYSET_DISTINCT: bool = true;

/// `.order_by(self.kwargs.get("order_by", "-created_at"))` (`:221,298,567`):
/// URL kwargs never carry `order_by`, so the default always applies
/// (ported quirk 6).
pub const QUERYSET_DEFAULT_ORDER: &str = "-created_at";

// ---------------------------------------------------------------------------
// List annotations: cycle_id / link_count / attachment_count
// ---------------------------------------------------------------------------

/// `cycle_id` (`views/issue.py:374-378`): the newest live `cycle_issues`
/// row for this issue. `deleted_at IS NULL` renders twice — manager scope
/// plus the explicit `deleted_at__isnull=True` (ported quirk 2, verified by
/// rendering the queryset through Django 4.2.30).
pub fn cycle_id_sql() -> String {
    "(SELECT cycle_issue.\"cycle_id\" FROM \"cycle_issues\" cycle_issue WHERE (cycle_issue.\"deleted_at\" IS NULL AND cycle_issue.\"deleted_at\" IS NULL AND cycle_issue.\"issue_id\" = (\"issues\".\"id\")) ORDER BY cycle_issue.\"created_at\" DESC LIMIT 1) AS \"cycle_id\"".to_owned()
}

/// `link_count` (`:380-385`): live `issue_links` rows for this issue.
/// `IssueLink.objects` is the inherited soft-delete manager
/// (`db/mixins.py:56-68`); the inner `.order_by()` clears ordering.
pub fn link_count_sql() -> String {
    "(SELECT Count(issue_link.\"id\") AS \"count\" FROM \"issue_links\" issue_link WHERE (issue_link.\"deleted_at\" IS NULL AND issue_link.\"issue_id\" = (\"issues\".\"id\"))) AS \"link_count\"".to_owned()
}

/// `attachment_count` (`:387-395`): live `ISSUE_ATTACHMENT` file assets for
/// this issue (`FileAsset.EntityTypeContext`, `db/models/asset.py:33-34`).
pub fn attachment_count_sql() -> String {
    "(SELECT Count(file_asset.\"id\") AS \"count\" FROM \"file_assets\" file_asset WHERE (file_asset.\"deleted_at\" IS NULL AND file_asset.\"entity_type\" = 'ISSUE_ATTACHMENT' AND file_asset.\"issue_id\" = (\"issues\".\"id\"))) AS \"attachment_count\"".to_owned()
}

/// List annotation aliases in source order.
pub const LIST_ANNOTATIONS: &[&str] = &[
    "sub_issues_count",
    "cycle_id",
    "link_count",
    "attachment_count",
];

// ---------------------------------------------------------------------------
// work_item_list_filters: predicate compilation
// ---------------------------------------------------------------------------

/// `WORK_ITEM_LIST_FILTER_KEYS` (`utils/issue_filters.py:482`).
pub const WORK_ITEM_LIST_FILTER_KEYS: &[&str] = &[
    "state",
    "state_group",
    "parent",
    "labels",
    "priority",
    "assignees",
];

/// `VALID_PRIORITIES` (`utils/issue_filters.py:484`). Same values as
/// [`PRIORITY_ORDER`] (which serves the ordering `CASE`); the equality is
/// pinned by test so the two Python spellings cannot drift apart here.
pub const VALID_PRIORITIES: &[&str] = &["urgent", "high", "medium", "low", "none"];

/// Every predicate key this compiler can emit, in first-write order: the
/// four legacy leaves (via [`issue_filters_get`]) then the two
/// through-model pairs. Named for app-domain reuse — app
/// analytics/cycle/intake/issue/module/view import the same
/// `utils/issue_filters.py` util, and their ports can reuse these keys plus
/// [`filter_joins_sql`]/[`filter_where_sql`].
pub const COMPILED_PREDICATE_KEYS: &[&str] = &[
    "state__in",
    "state__group__in",
    "priority__in",
    "parent__isnull",
    "parent__in",
    "label_issue__label_id__in",
    "label_issue__deleted_at__isnull",
    "issue_assignee__assignee_id__in",
    "issue_assignee__deleted_at__isnull",
];

/// `IssueFilterError` (`utils/issue_filters.py:485-486`): `str(err)` is
/// user-facing and the view renders it as `{"error": str(e)}` at 400
/// (`views/issue.py:368-370`). [`WorkItemFilterError::body`] is that exact
/// body, via the shared [`ParamError`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkItemFilterError {
    /// A resolution/validation failure; the message is the Python `str(e)`.
    #[error("{0}")]
    Message(String),
    /// The db kernel's `DateOverflow`, unreachable here (no date leaf can
    /// appear in `normalized`) — mapped the way the kernel documents: the
    /// handler answers 500, as Python's uncaught `OverflowError` would.
    #[error("relative date magnitude overflows the calendar")]
    DateOverflow,
}

impl WorkItemFilterError {
    fn message(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }

    /// The exact 400 body Django renders (`{"error": ...}`).
    pub fn body(&self) -> String {
        match self {
            Self::Message(text) => ParamError::error(text.clone()).body(),
            Self::DateOverflow => {
                ParamError::error("relative date magnitude overflows the calendar").body()
            }
        }
    }
}

/// `_split_tokens` (`utils/issue_filters.py:489-493`): comma-split, strip,
/// drop empties; `None` yields no tokens.
pub fn split_tokens(raw: Option<&str>) -> Vec<String> {
    match raw {
        None => Vec::new(),
        Some(text) => text
            .split(',')
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(str::to_owned)
            .collect(),
    }
}

/// `_is_uuid` (`utils/issue_filters.py:495-500`): `uuid.UUID(str(value))`
/// accepts hyphenated, simple, braced and `urn:` forms, exactly like
/// `Uuid::parse_str`.
pub fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok()
}

/// Lowercase-sort with a deterministic tiebreak (ported quirk 10): Python
/// sorts a hash-ordered *set* by `str.lower`, so ties across names that
/// differ only by case render in an arbitrary order; the port orders ties
/// by the original string.
fn sort_valid_names(names: &std::collections::HashSet<String>) -> Vec<String> {
    let mut valid: Vec<String> = names.iter().cloned().collect();
    valid.sort_by(|a, b| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
    valid
}

/// `_resolve_states` (`utils/issue_filters.py:503-520`): UUID tokens pass
/// through verbatim; names resolve case-insensitively against the
/// project-state rows the caller loaded with [`states_lookup_sql`].
/// Duplicate names resolve repeatedly (Python extends per name, unknown or
/// not, in input order).
pub fn resolve_states(
    tokens: &[String],
    states: &[(String, String)],
) -> Result<Vec<String>, WorkItemFilterError> {
    let mut ids: Vec<String> = tokens.iter().filter(|t| is_uuid(t)).cloned().collect();
    let names: Vec<&String> = tokens.iter().filter(|t| !is_uuid(t)).collect();
    if !names.is_empty() {
        let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
        for (id, name) in states {
            by_name
                .entry(name.to_lowercase())
                .or_default()
                .push(id.clone());
        }
        let unknown: Vec<String> = names
            .iter()
            .filter(|name| !by_name.contains_key(&name.to_lowercase()))
            .map(|name| (*name).clone())
            .collect();
        if !unknown.is_empty() {
            let known: std::collections::HashSet<String> =
                states.iter().map(|(_, name)| name.clone()).collect();
            let valid = sort_valid_names(&known).join(", ");
            return Err(WorkItemFilterError::message(format!(
                "Unknown state name(s): {}. Valid states: {valid}.",
                unknown.join(", ")
            )));
        }
        for name in names {
            if let Some(found) = by_name.get(&name.to_lowercase()) {
                ids.extend(found.iter().cloned());
            }
        }
    }
    Ok(ids)
}

/// `_resolve_labels` (`utils/issue_filters.py:522-543`): UUID tokens pass
/// through verbatim; names resolve case-insensitively against the scoped
/// label rows the caller loaded with [`labels_lookup_sql`]. An empty scope
/// renders the `(none)` fallback (ported quirk 9).
pub fn resolve_labels(
    tokens: &[String],
    labels: &[(String, String)],
) -> Result<Vec<String>, WorkItemFilterError> {
    let mut ids: Vec<String> = tokens.iter().filter(|t| is_uuid(t)).cloned().collect();
    let names: Vec<&String> = tokens.iter().filter(|t| !is_uuid(t)).collect();
    if !names.is_empty() {
        let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
        for (id, name) in labels {
            by_name
                .entry(name.to_lowercase())
                .or_default()
                .push(id.clone());
        }
        let unknown: Vec<String> = names
            .iter()
            .filter(|name| !by_name.contains_key(&name.to_lowercase()))
            .map(|name| (*name).clone())
            .collect();
        if !unknown.is_empty() {
            let known: std::collections::HashSet<String> =
                labels.iter().map(|(_, name)| name.clone()).collect();
            let mut valid = sort_valid_names(&known).join(", ");
            if valid.is_empty() {
                valid = "(none)".to_owned();
            }
            return Err(WorkItemFilterError::message(format!(
                "Unknown label name(s): {}. Valid labels: {valid}.",
                unknown.join(", ")
            )));
        }
        for name in names {
            if let Some(found) = by_name.get(&name.to_lowercase()) {
                ids.extend(found.iter().cloned());
            }
        }
    }
    Ok(ids)
}

/// Split one parent token (`utils/issue_filters.py:551-556`):
/// `rpartition("-")` with a non-empty identifier and an ASCII-digit
/// sequence. Python's `sequence.isdigit()` is Unicode-aware and `int()`
/// then raises on non-ASCII digits (an uncaught 500); the port answers 400
/// there — unreachable from real identifiers, and a one-line divergence.
/// Overflowing magnitudes stay valid-but-unknown: Postgres widens the
/// comparison and matches nothing, so Python reports "Unknown parent".
fn split_parent_token(token: &str) -> Result<Option<(&str, i64)>, WorkItemFilterError> {
    if is_uuid(token) {
        return Ok(None);
    }
    let invalid = || {
        WorkItemFilterError::message(format!(
            "Invalid parent '{token}': expected an issue UUID, an identifier like PROJ-123, or null."
        ))
    };
    let Some((identifier, sequence)) = token.rsplit_once('-') else {
        return Err(invalid());
    };
    if identifier.is_empty() || sequence.is_empty() || !sequence.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(invalid());
    }
    match sequence.parse::<i64>() {
        Ok(number) => Ok(Some((identifier, number))),
        // `int()` is unbounded in Python and the comparison then matches
        // nothing: "Unknown parent", not "Invalid parent".
        Err(_) => Err(WorkItemFilterError::message(format!(
            "Unknown parent issue '{token}'."
        ))),
    }
}

/// `_resolve_parents` (`utils/issue_filters.py:545-571`): UUID tokens pass
/// through verbatim; `PROJ-123` identifiers resolve through `resolve`, which
/// the handler implements with [`parent_lookup_sql`] (returning the row id
/// or `None`). The identifier match is `iexact`; `resolve` receives the raw
/// identifier token and the parsed sequence number.
pub fn resolve_parents(
    tokens: &[String],
    resolve: impl Fn(&str, i64) -> Option<String>,
) -> Result<Vec<String>, WorkItemFilterError> {
    let mut ids = Vec::with_capacity(tokens.len());
    for token in tokens {
        match split_parent_token(token)? {
            None => ids.push(token.clone()),
            Some((identifier, sequence)) => match resolve(identifier, sequence) {
                Some(id) => ids.push(id),
                None => {
                    return Err(WorkItemFilterError::message(format!(
                        "Unknown parent issue '{token}'."
                    )));
                }
            },
        }
    }
    Ok(ids)
}

/// `work_item_list_filters` (`utils/issue_filters.py:573-654`): resolve the
/// six agent-facing keys into kwargs for `Issue.objects.filter`, in Python
/// validation order (state, state_group, priority, parent, labels,
/// assignees — the first failure wins).
///
/// `params` is the single-value query map (`QueryDict.get` takes the *last*
/// value on repeats; the handler collapses repeats that way). `states` and
/// `labels` are the `(id, name)` rows from [`states_lookup_sql`] and
/// [`labels_lookup_sql`]; `resolve_parent` answers one
/// [`parent_lookup_sql`] per identifier token. `today` feeds the db kernel
/// (no date leaf can appear in `normalized`, so it is never read).
/// Absent or blank keys are ignored: empty params yield an empty filter.
pub fn compile_work_item_filters(
    params: &HashMap<String, String>,
    states: &[(String, String)],
    labels: &[(String, String)],
    resolve_parent: impl Fn(&str, i64) -> Option<String>,
    today: NaiveDate,
) -> Result<IssueFilter, WorkItemFilterError> {
    let mut normalized: HashMap<String, String> = HashMap::new();

    let tokens = split_tokens(params.get("state").map(String::as_str));
    if !tokens.is_empty() {
        normalized.insert(
            "state".to_owned(),
            resolve_states(&tokens, states)?.join(","),
        );
    }

    let groups: Vec<String> = split_tokens(params.get("state_group").map(String::as_str))
        .iter()
        .map(|group| group.to_lowercase())
        .collect();
    if !groups.is_empty() {
        let invalid: Vec<&str> = groups
            .iter()
            .filter(|group| !STATE_ORDER.contains(&group.as_str()))
            .map(String::as_str)
            .collect();
        if !invalid.is_empty() {
            return Err(WorkItemFilterError::message(format!(
                "Unknown state group(s): {}. Valid groups: {}.",
                invalid.join(", "),
                STATE_ORDER.join(", ")
            )));
        }
        normalized.insert("state_group".to_owned(), groups.join(","));
    }

    let priorities: Vec<String> = split_tokens(params.get("priority").map(String::as_str))
        .iter()
        .map(|priority| priority.to_lowercase())
        .collect();
    if !priorities.is_empty() {
        let invalid: Vec<&str> = priorities
            .iter()
            .filter(|priority| !VALID_PRIORITIES.contains(&priority.as_str()))
            .map(String::as_str)
            .collect();
        if !invalid.is_empty() {
            return Err(WorkItemFilterError::message(format!(
                "Unknown priority value(s): {}. Valid priorities: {}.",
                invalid.join(", "),
                VALID_PRIORITIES.join(", ")
            )));
        }
        normalized.insert("priority".to_owned(), priorities.join(","));
    }

    let parents = split_tokens(params.get("parent").map(String::as_str));
    if !parents.is_empty() {
        let nulls = parents
            .iter()
            .filter(|p| p.to_lowercase() == "null" || p.to_lowercase() == "none")
            .count();
        if nulls > 0 && nulls != parents.len() {
            return Err(WorkItemFilterError::message(
                "parent=null cannot be combined with specific parent issues.",
            ));
        }
        // `filter_parent` spells "top-level only" as the literal "None".
        let value = if nulls > 0 {
            "None".to_owned()
        } else {
            resolve_parents(&parents, resolve_parent)?.join(",")
        };
        normalized.insert("parent".to_owned(), value);
    }

    let mut filters = if normalized.is_empty() {
        IssueFilter::default()
    } else {
        issue_filters_get(&normalized, "", today).map_err(|_| WorkItemFilterError::DateOverflow)?
    };

    let tokens = split_tokens(params.get("labels").map(String::as_str));
    if !tokens.is_empty() {
        // Through-model in one join so a soft-deleted label link never
        // matches (`filter_labels` would join M2M and through separately).
        filters.set(
            "label_issue__label_id__in".to_owned(),
            FilterValue::Strings(resolve_labels(&tokens, labels)?),
        );
        filters.set(
            "label_issue__deleted_at__isnull".to_owned(),
            FilterValue::Flag(true),
        );
    }

    let tokens = split_tokens(params.get("assignees").map(String::as_str));
    if !tokens.is_empty() {
        let invalid: Vec<&str> = tokens
            .iter()
            .filter(|token| !is_uuid(token))
            .map(String::as_str)
            .collect();
        if !invalid.is_empty() {
            return Err(WorkItemFilterError::message(format!(
                "Invalid assignee id(s): {}. Expected user UUIDs.",
                invalid.join(", ")
            )));
        }
        filters.set(
            "issue_assignee__assignee_id__in".to_owned(),
            FilterValue::Strings(tokens),
        );
        filters.set(
            "issue_assignee__deleted_at__isnull".to_owned(),
            FilterValue::Flag(true),
        );
    }

    Ok(filters)
}

/// `INNER JOIN`s for the through-model predicates in `filters`, in `Q`
/// key-sorted order (assignees before labels — verified by rendering
/// through Django 4.2.30). Aliased `filter_labels`/`filter_assignees` so
/// they never collide with the ordering `MAX` joins. Handlers splice these
/// after the scope joins and *before* the parent self-join: filter joins
/// are built during `.filter()`, `select_related` joins at compile time.
pub fn filter_joins_sql(filters: &IssueFilter) -> String {
    let mut joins = Vec::new();
    if filters
        .predicates()
        .iter()
        .any(|(key, _)| key.starts_with("issue_assignee__"))
    {
        joins.push(
            "INNER JOIN \"issue_assignees\" filter_assignees ON (\"issues\".\"id\" = filter_assignees.\"issue_id\")",
        );
    }
    if filters
        .predicates()
        .iter()
        .any(|(key, _)| key.starts_with("label_issue__"))
    {
        joins.push(
            "INNER JOIN \"issue_labels\" filter_labels ON (\"issues\".\"id\" = filter_labels.\"issue_id\")",
        );
    }
    joins.join(" ")
}

/// `WHERE` conjunction for a compiled [`IssueFilter`]. Django's `Q`
/// sorts single-call kwargs by lookup string (`Q.children = [*args,
/// *sorted(kwargs.items())]`), so predicates render in *key* order, not
/// insertion order (verified by rendering through Django 4.2.30).
/// List values bind as one plural placeholder each (`:states`, …) that the
/// handler expands. Unknown keys cannot occur (the compiler is closed over
/// [`COMPILED_PREDICATE_KEYS`]) and are skipped defensively.
pub fn filter_where_sql(filters: &IssueFilter) -> String {
    let mut predicates: Vec<&(String, FilterValue)> = filters.predicates().iter().collect();
    predicates.sort_by(|a, b| a.0.cmp(&b.0));
    let mut parts = Vec::new();
    for (key, value) in predicates {
        let part = match (key.as_str(), value) {
            ("state__in", FilterValue::Uuids(ids)) if !ids.is_empty() => {
                Some("\"states\".\"id\" IN (:states)".to_owned())
            }
            ("state__group__in", FilterValue::Strings(groups)) if !groups.is_empty() => {
                Some("\"states\".\"group\" IN (:state_groups)".to_owned())
            }
            ("priority__in", FilterValue::Strings(priorities)) if !priorities.is_empty() => {
                Some("\"issues\".\"priority\" IN (:priorities)".to_owned())
            }
            ("parent__isnull", FilterValue::Flag(true)) => {
                Some("\"issues\".\"parent_id\" IS NULL".to_owned())
            }
            ("parent__isnull", FilterValue::Flag(false)) => {
                Some("\"issues\".\"parent_id\" IS NOT NULL".to_owned())
            }
            ("parent__in", FilterValue::Uuids(ids)) if !ids.is_empty() => {
                Some("\"issues\".\"parent_id\" IN (:parents)".to_owned())
            }
            ("label_issue__label_id__in", FilterValue::Strings(ids)) if !ids.is_empty() => {
                Some("filter_labels.\"label_id\" IN (:label_ids)".to_owned())
            }
            ("label_issue__deleted_at__isnull", FilterValue::Flag(true)) => {
                Some("filter_labels.\"deleted_at\" IS NULL".to_owned())
            }
            ("label_issue__deleted_at__isnull", FilterValue::Flag(false)) => {
                Some("filter_labels.\"deleted_at\" IS NOT NULL".to_owned())
            }
            ("issue_assignee__assignee_id__in", FilterValue::Strings(ids)) if !ids.is_empty() => {
                Some("filter_assignees.\"assignee_id\" IN (:assignee_ids)".to_owned())
            }
            ("issue_assignee__deleted_at__isnull", FilterValue::Flag(true)) => {
                Some("filter_assignees.\"deleted_at\" IS NULL".to_owned())
            }
            ("issue_assignee__deleted_at__isnull", FilterValue::Flag(false)) => {
                Some("filter_assignees.\"deleted_at\" IS NOT NULL".to_owned())
            }
            _ => None,
        };
        if let Some(part) = part {
            parts.push(part);
        }
    }
    parts.join(" AND ")
}

// ---------------------------------------------------------------------------
// Resolution lookups (the SELECTs behind _resolve_states/_labels/_parents)
// ---------------------------------------------------------------------------

/// `State.objects.filter(project_id=...).values_list("id", "name")`
/// (`utils/issue_filters.py:508`): live, non-triage states in `sequence`
/// order (`StateManager`, `db/models/state.py:79-84`; `Meta.ordering`,
/// `:129`). The `exclude(group="triage")` hits a concrete non-null field,
/// so no `NULL` guard renders (unlike the manager-scope exclusion).
pub fn states_lookup_sql() -> String {
    "SELECT \"states\".\"id\", \"states\".\"name\" FROM \"states\" WHERE (\"states\".\"deleted_at\" IS NULL AND NOT (\"states\".\"group\" = 'triage') AND \"states\".\"project_id\" = :project_id) ORDER BY \"states\".\"sequence\" ASC".to_owned()
}

/// `Label.objects.filter(Q(project_id=...) | Q(project__isnull=True,
/// workspace__slug=...)).values_list("id", "name")` (`:528-529`):
/// project labels plus workspace-level labels (null project) attachable in
/// every project. The `OR` keeps the `workspaces` join `LEFT OUTER`;
/// `labels.project_id IS NULL` needs no `projects` join. Newest first
/// (`Meta.ordering`, `db/models/label.py:44`).
pub fn labels_lookup_sql() -> String {
    "SELECT \"labels\".\"id\", \"labels\".\"name\" FROM \"labels\" LEFT OUTER JOIN \"workspaces\" ON (\"labels\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"labels\".\"deleted_at\" IS NULL AND (\"labels\".\"project_id\" = :project_id OR (\"labels\".\"project_id\" IS NULL AND \"workspaces\".\"slug\" = :workspace_slug))) ORDER BY \"labels\".\"created_at\" DESC".to_owned()
}

/// `Issue.issue_objects.filter(workspace__slug=..., project__identifier
/// __iexact=..., sequence_id=...).values_list("id", flat=True).first()`
/// (`:554-561`): full manager scope (a triage/archived/draft parent never
/// resolves), case-insensitive identifier match, newest row on ties
/// (`Meta.ordering`, first). Single-call kwargs render `Q`-sorted
/// (`project__identifier__iexact`, `sequence_id`, `workspace__slug`).
pub fn parent_lookup_sql() -> String {
    format!(
        "SELECT \"issues\".\"id\" FROM \"issues\" {} WHERE ({} AND UPPER(\"projects\".\"identifier\"::text) = UPPER(:project_identifier) AND \"issues\".\"sequence_id\" = :sequence AND \"workspaces\".\"slug\" = :workspace_slug) ORDER BY \"issues\".\"created_at\" DESC LIMIT 1",
        parent_lookup_joins_sql(),
        ISSUE_MANAGER_WHERE,
    )
}

/// Joins for [`parent_lookup_sql`]: the manager scope (`states`, `projects`)
/// plus `workspaces` for the slug predicate.
fn parent_lookup_joins_sql() -> String {
    [
        "LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\")",
        "INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\")",
        "INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\")",
    ]
    .join(" ")
}

// ---------------------------------------------------------------------------
// List ordering (views/issue.py:361-437)
// ---------------------------------------------------------------------------

/// The resolved list ordering: annotation fragment plus `ORDER BY` text.
/// Every branch *replaces* the queryset order (ported quirk 7: no
/// tiebreak, unlike the app ordering) — the v1 envelope echoes no
/// `order_by`, so there is no rewritten param.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderSpec {
    /// A renderable ordering.
    Ordered {
        /// `SELECT` annotation fragment (`... AS "alias"`), if any.
        annotation_sql: Option<String>,
        /// Extra `JOIN`s (the `MAX` branch only).
        joins_sql: String,
        /// `ORDER BY` fragment (without the keywords).
        order_by_sql: String,
        /// The `MAX` branch aggregates over a join: Django adds `GROUP BY`
        /// over the whole select list plus `.distinct()` handling.
        requires_grouping: bool,
    },
    /// Django would fan out with new joins here (multi-level `__` paths
    /// beyond the `select_related` relations); the handler answers 500
    /// (ported quirk 12).
    Unsupported,
}

/// `order_by` default of the list `GET`
/// (`request.GET.get("order_by", "-created_at")`, `views/issue.py:366`).
pub const LIST_DEFAULT_ORDER_PARAM: &str = "-created_at";

/// The priority `CASE` *select* annotation (`views/issue.py:407-412`):
/// `WHEN "issues"."priority" = '<p>' THEN <i> ... ELSE NULL END AS
/// "priority_order"`. `order` is forward for `priority`, reversed for
/// `-priority` (both branches then sort ascending).
pub fn priority_annotation_sql(order: &[&str]) -> String {
    let mut cases = String::new();
    for (index, priority) in order.iter().enumerate() {
        cases.push_str(&format!(
            "WHEN \"issues\".\"priority\" = '{priority}' THEN {index} "
        ));
    }
    format!("CASE {cases}ELSE NULL END AS \"priority_order\"")
}

/// The priority `CASE` as Django inlines it into `ORDER BY`: same `WHEN`s
/// with parenthesized conditions (`ORDER BY CASE WHEN (...) THEN ... END
/// ASC`, verified by rendering through Django 4.2.30).
pub fn priority_order_sql(order: &[&str]) -> String {
    let mut cases = String::new();
    for (index, priority) in order.iter().enumerate() {
        cases.push_str(&format!(
            "WHEN (\"issues\".\"priority\" = '{priority}') THEN {index} "
        ));
    }
    format!("CASE {cases}ELSE NULL END ASC")
}

/// The state-group `CASE` *select* annotation (`:421-427`): `WHEN
/// "states"."group" = '<g>' THEN <i> ... ELSE 7 END AS "state_order"`.
/// `order` is forward for ascending params, reversed for descending ones —
/// the v1 branch test matches the branch list exactly, so unlike the app
/// port the `[::-1]` alternative is live.
pub fn state_order_annotation_sql(order: &[&str]) -> String {
    let mut cases = String::new();
    for (index, group) in order.iter().enumerate() {
        cases.push_str(&format!(
            "WHEN \"states\".\"group\" = '{group}' THEN {index} "
        ));
    }
    format!("CASE {cases}ELSE {} END AS \"state_order\"", order.len())
}

/// The state-group `CASE` as Django inlines it into `ORDER BY`.
pub fn state_order_order_sql(order: &[&str]) -> String {
    let mut cases = String::new();
    for (index, group) in order.iter().enumerate() {
        cases.push_str(&format!(
            "WHEN (\"states\".\"group\" = '{group}') THEN {index} "
        ));
    }
    format!("CASE {cases}ELSE {} END ASC", order.len())
}

/// Reverse a static order list (the `-priority` / `-state__*` branches).
fn reversed<'a>(order: &[&'a str]) -> Vec<&'a str> {
    order.iter().rev().copied().collect()
}

/// `MAX`-orderable relations (`:430-437`): the m2m path, its through
/// table, its target table/alias, and the aggregated column. Both joins
/// are `LEFT OUTER` (verified by rendering through Django 4.2.30), so
/// issues without labels/assignees sort with a `NULL` max.
pub const MAX_ORDER_FIELDS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "labels__name",
        "issue_labels",
        "labels",
        "order_labels",
        "name",
    ),
    (
        "assignees__first_name",
        "issue_assignees",
        "users",
        "order_users",
        "first_name",
    ),
];

/// Annotation aliases orderable by name on the default branch: the list
/// annotations are in scope for `.order_by(...)`.
pub const ORDERABLE_ANNOTATIONS: &[&str] = &[
    "sub_issues_count",
    "cycle_id",
    "link_count",
    "attachment_count",
];

/// Concrete `issues` columns orderable by bare name (F18-05, 34 columns).
pub const ORDERABLE_ISSUE_COLUMNS: &[&str] = &[
    "created_at",
    "updated_at",
    "id",
    "name",
    "description_json",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "created_by_id",
    "parent_id",
    "project_id",
    "state_id",
    "updated_by_id",
    "workspace_id",
    "description_html",
    "description_stripped",
    "completed_at",
    "sort_order",
    "point",
    "archived_at",
    "is_draft",
    "external_id",
    "external_source",
    "description_binary",
    "estimate_point_id",
    "type_id",
    "deleted_at",
    "git_work_branch",
    "assigned_pod_id",
    "workpad",
    "created_via",
    "agent_executor",
    "complexity_score",
];

/// Django field names orderable by bare name, mapped to their columns
/// (`order_by("state")` orders by `state_id`; `pk` is `id`).
pub const ORDERABLE_FIELD_ALIASES: &[(&str, &str)] = &[
    ("pk", "id"),
    ("parent", "parent_id"),
    ("state", "state_id"),
    ("project", "project_id"),
    ("workspace", "workspace_id"),
    ("created_by", "created_by_id"),
    ("updated_by", "updated_by_id"),
    ("estimate_point", "estimate_point_id"),
    ("type", "type_id"),
    ("assigned_pod", "assigned_pod_id"),
];

/// Relations traversable one level on the default branch, mapped to the
/// alias the `select_related` joins already provide (no new join needed).
pub const ORDERABLE_RELATIONS: &[(&str, &str)] = &[
    ("state", "states"),
    ("project", "projects"),
    ("workspace", "workspaces"),
    ("parent", "parent_issue"),
];

/// A syntactically valid identifier segment (Django field path); anything
/// else is not a field path at all.
fn is_identifier_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Default-branch ordering (`views/issue.py:436-437`):
/// `.order_by(order_by_param)`. Covers annotation names, `?` (random),
/// bare columns/fields, and one-level `select_related` traversals;
/// anything needing new joins is [`OrderSpec::Unsupported`].
fn default_order_spec(param: &str) -> OrderSpec {
    let (descending, column) = match param.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, param),
    };
    let direction = if descending { "DESC" } else { "ASC" };
    let ordered = |order_by_sql: String| OrderSpec::Ordered {
        annotation_sql: None,
        joins_sql: String::new(),
        order_by_sql,
        requires_grouping: false,
    };
    if column == "?" && !descending {
        return ordered("RANDOM()".to_owned());
    }
    if ORDERABLE_ANNOTATIONS.contains(&column) {
        return ordered(format!("\"{column}\" {direction}"));
    }
    if !column.contains("__") {
        if ORDERABLE_ISSUE_COLUMNS.contains(&column) {
            return ordered(format!("\"issues\".\"{column}\" {direction}"));
        }
        if let Some((_, target)) = ORDERABLE_FIELD_ALIASES
            .iter()
            .find(|(name, _)| *name == column)
        {
            return ordered(format!("\"issues\".\"{target}\" {direction}"));
        }
        // Unknown bare name: Django raises `FieldError` (500). The port
        // splices the qualified identifier (charset-checked) so the
        // database errors the same way — same outcome class, no injection.
        if is_identifier_segment(column) {
            return ordered(format!("\"issues\".\"{column}\" {direction}"));
        }
        return OrderSpec::Unsupported;
    }
    let mut parts = column.split("__");
    let (Some(relation), Some(field), None) = (parts.next(), parts.next(), parts.next()) else {
        return OrderSpec::Unsupported;
    };
    match ORDERABLE_RELATIONS
        .iter()
        .find(|(name, _)| *name == relation)
    {
        Some((_, alias)) if is_identifier_segment(field) => {
            ordered(format!("\"{alias}\".\"{field}\" {direction}"))
        }
        _ => OrderSpec::Unsupported,
    }
}

/// Port of the list ordering chain (`views/issue.py:404-437`).
pub fn order_spec(order_by_param: &str) -> OrderSpec {
    if order_by_param == "priority" || order_by_param == "-priority" {
        let order: Vec<&str> = if order_by_param == "priority" {
            PRIORITY_ORDER.to_vec()
        } else {
            reversed(PRIORITY_ORDER)
        };
        return OrderSpec::Ordered {
            annotation_sql: Some(priority_annotation_sql(&order)),
            joins_sql: String::new(),
            order_by_sql: priority_order_sql(&order),
            requires_grouping: false,
        };
    }
    if matches!(
        order_by_param,
        "state__name" | "state__group" | "-state__name" | "-state__group"
    ) {
        // Ported bug 1: `state__name` takes this branch too and orders by
        // group, never by name.
        let ascending = order_by_param == "state__name" || order_by_param == "state__group";
        let order: Vec<&str> = if ascending {
            STATE_ORDER.to_vec()
        } else {
            reversed(STATE_ORDER)
        };
        return OrderSpec::Ordered {
            annotation_sql: Some(state_order_annotation_sql(&order)),
            joins_sql: String::new(),
            order_by_sql: state_order_order_sql(&order),
            requires_grouping: false,
        };
    }
    let stripped = order_by_param.strip_prefix('-').unwrap_or(order_by_param);
    if let Some((_, through, target, alias, column)) = MAX_ORDER_FIELDS
        .iter()
        .find(|(name, _, _, _, _)| *name == stripped)
    {
        // `Max(order_by_param[1::] if startswith("-") else ...)` — strips
        // exactly one leading `-` (ported quirk 8). Django inlines the
        // aggregate into ORDER BY and groups by the select list.
        let descending = order_by_param.starts_with('-');
        let direction = if descending { "DESC" } else { "ASC" };
        let through_alias = format!("order_{through}");
        return OrderSpec::Ordered {
            annotation_sql: Some(format!("MAX(\"{alias}\".\"{column}\") AS \"max_values\"")),
            joins_sql: format!(
                "LEFT OUTER JOIN \"{through}\" {through_alias} ON (\"issues\".\"id\" = {through_alias}.\"issue_id\") LEFT OUTER JOIN \"{target}\" \"{alias}\" ON ({through_alias}.\"{fk}\" = \"{alias}\".\"id\")",
                fk = if *target == "labels" { "label_id" } else { "assignee_id" },
            ),
            order_by_sql: format!("MAX(\"{alias}\".\"{column}\") {direction}"),
            requires_grouping: true,
        };
    }
    default_order_spec(order_by_param)
}

// ---------------------------------------------------------------------------
// BasePaginator.paginate call surface (views/issue.py:439-444)
// ---------------------------------------------------------------------------

/// `paginate` defaults (`utils/paginator.py:660-661`): `default_per_page`
/// and `max_per_page` are both 1000 on this path. Parsing reuses
/// [`parse_per_page`](crate::app_issues::params::parse_per_page), whose
/// error bodies are pinned against F18-06 below.
pub const PAGINATE_DEFAULT_PER_PAGE: i64 = 1000;
/// See [`PAGINATE_DEFAULT_PER_PAGE`].
pub const PAGINATE_MAX_PER_PAGE: i64 = 1000;

/// Cursor query param name (`BasePaginator.cursor_name`, `:639`).
pub const CURSOR_PARAM: &str = "cursor";

/// Cursor wire format (`Cursor.__str__`, `paginator.py:32-33`).
pub const CURSOR_FORMAT: &str = "value:offset:is_prev";

/// Default cursor when `?cursor=` is absent
/// (`f"{per_page}:0:0"`, `paginator.py:668`).
pub fn default_cursor(per_page: i64) -> String {
    format!("{per_page}:0:0")
}

/// `BasePaginator.paginate` envelope keys (`paginator.py:717-732`), in
/// response order. The v1 list passes no grouping, controller or stats, so
/// `grouped_by`/`sub_grouped_by`/`extra_stats` render `null`.
pub const PAGINATE_ENVELOPE_KEYS: &[&str] = &[
    "grouped_by",
    "sub_grouped_by",
    "total_count",
    "next_cursor",
    "prev_cursor",
    "next_page_results",
    "prev_page_results",
    "count",
    "total_pages",
    "total_results",
    "extra_stats",
    "results",
];

/// `total_count_queryset` (`views/issue.py:397-399`): the manager scope
/// plus the compiled filters — with `.distinct()` only when filters are
/// present (ported quirk 4). Returns the `WHERE` and whether the count
/// query must be `COUNT(DISTINCT ...)`.
pub fn total_count_query(filters: &IssueFilter) -> (String, bool) {
    let mut where_clause = format!("{} AND {}", ISSUE_MANAGER_WHERE, project_scope_sql());
    if filters.is_empty() {
        return (where_clause, false);
    }
    let extra = filter_where_sql(filters);
    if !extra.is_empty() {
        where_clause.push_str(" AND ");
        where_clause.push_str(&extra);
    }
    (where_clause, true)
}

/// Cursor/offset math lives in the shared `pidash_api::paginator` kernel
/// (`Cursor::from_string`, `offset_window`, `next_cursor`/`prev_cursor`,
/// `max_hits`, `PageResponse` envelope); handlers use it directly. This
/// module owns only the v1 call surface above.
pub const PAGINATOR_KERNEL: &str = "pidash_api::paginator";

// ---------------------------------------------------------------------------
// Detail / upsert / patch / delete lookups
// ---------------------------------------------------------------------------

/// External-id early return (`views/issue.py:344-359`): plain `objects`
/// scope (soft-delete only — no triage/archived/draft exclusions), exact
/// `external_id` + `external_source` match. The `.get()` 404 contract is
/// handler-owned.
pub fn external_id_lookup_where() -> String {
    "\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"external_id\" = :external_id AND \"issues\".\"external_source\" = :external_source AND \"issues\".\"project_id\" = :project_id AND \"workspaces\".\"slug\" = :slug".to_owned()
}

/// Joins for [`external_id_lookup_where`]: only `workspaces` (no manager
/// joins under plain `objects`).
pub fn external_id_lookup_joins_sql() -> String {
    "INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\")".to_owned()
}

/// Workspace `get` inline lookup (`:250-260`): manager scope plus the
/// identifier triple (ported quirk 5: no `select_related`, no `distinct`).
pub fn workspace_get_lookup_where() -> String {
    format!(
        "{} AND \"projects\".\"identifier\" = :project_identifier AND \"issues\".\"sequence_id\" = :sequence AND \"workspaces\".\"slug\" = :slug",
        ISSUE_MANAGER_WHERE
    )
}

/// Detail `get` inline lookup (`:600-605`): manager scope plus the pk
/// triple (ported quirk 5).
pub fn detail_get_lookup_where() -> String {
    format!(
        "{} AND \"issues\".\"id\" = :pk AND \"issues\".\"project_id\" = :project_id AND \"workspaces\".\"slug\" = :slug",
        ISSUE_MANAGER_WHERE
    )
}

/// Joins shared by the workspace/detail inline `.get()` lookups: manager
/// scope plus `workspaces` (predicate order follows the fixture).
pub fn inline_lookup_joins_sql() -> String {
    [
        "LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\")",
        "INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\")",
        "INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\")",
    ]
    .join(" ")
}

/// Inline `.get()` lookups carry no explicit `order_by`, so
/// `Meta.ordering = ("-created_at",)` (`db/models/issue.py:254`) applies.
pub const LOOKUP_ORDER_SQL: &str = "\"issues\".\"created_at\" DESC";

/// `put` upsert lookup (`views/issue.py:654-659`): plain `objects` scope
/// (soft-delete only) on the external key pair. Single-call kwargs render
/// `Q`-sorted (`external_id`, `external_source`, `project_id`,
/// `workspace__slug`). `Issue.DoesNotExist` takes the create arm
/// (`:699-767`); the update arm (`:670-697`) and the
/// `Project.objects.get(pk=...)` preload (`:647`) are handler-owned.
pub fn put_upsert_lookup_where() -> String {
    "\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"external_id\" = :external_id AND \"issues\".\"external_source\" = :external_source AND \"issues\".\"project_id\" = :project_id AND \"workspaces\".\"slug\" = :slug".to_owned()
}

/// Joins for [`put_upsert_lookup_where`]: only `workspaces`.
pub fn put_upsert_lookup_joins_sql() -> String {
    external_id_lookup_joins_sql()
}

/// Missing-keys message (`:763-767`): `external_id and external_source
/// are required`, rendered as `{"error": ...}` at 400.
pub const PUT_MISSING_KEYS_MESSAGE: &str = "external_id and external_source are required";

/// Missing-keys 400 body, via the shared [`ParamError`].
pub fn put_missing_keys_body() -> String {
    ParamError::error(PUT_MISSING_KEYS_MESSAGE).body()
}

/// Post-create refetch (`:718-722`): `.filter(...).first()` — same scope,
/// newest row (`Meta.ordering`), one row. Single-call kwargs render
/// `Q`-sorted (`pk`, `project_id`, `workspace__slug`).
pub fn put_refetch_where() -> String {
    "\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"id\" = :pk AND \"issues\".\"project_id\" = :project_id AND \"workspaces\".\"slug\" = :slug".to_owned()
}

/// `.first()` renders `ORDER BY ... DESC LIMIT 1`.
pub const PUT_REFETCH_TAIL_SQL: &str = "ORDER BY \"issues\".\"created_at\" DESC LIMIT 1";

/// `patch` lookup (`views/issue.py:796`): plain `objects` `.get()` on the
/// pk triple. (The external-id conflict `.exists()` at `:821-829` has no
/// F18-06 pin and ships with the handler.)
pub fn patch_lookup_where() -> String {
    "\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"id\" = :pk AND \"issues\".\"project_id\" = :project_id AND \"workspaces\".\"slug\" = :slug".to_owned()
}

/// `delete` lookup (`views/issue.py:884`): the same pk-triple `.get()`.
/// The admin/creator check (`:885-897`, `ProjectMember` role 20 or creator)
/// is guard-owned (PIDASHCONV-671/F18-09).
pub fn delete_lookup_where() -> String {
    patch_lookup_where()
}

/// Joins shared by the patch/delete/patch-refetch `.get()` lookups: only
/// `workspaces` under plain `objects`.
pub fn pk_lookup_joins_sql() -> String {
    external_id_lookup_joins_sql()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_issues::params::parse_per_page;
    use serde_json::Value;

    fn fixture_text() -> String {
        let path = format!(
            "{}/../../fixtures/v1_work_items/queries/F18-06.list_detail.json",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(&path).expect("fixture exists")
    }

    fn fixture() -> Value {
        serde_json::from_str(&fixture_text()).expect("fixture parses")
    }

    fn unit<'a>(fixture: &'a Value, name: &str) -> &'a Value {
        fixture
            .get("units")
            .and_then(|units| units.get(name))
            .unwrap_or_else(|| panic!("fixture lacks unit {name}"))
    }

    fn unit_sql<'a>(fixture: &'a Value, name: &str) -> &'a str {
        unit(fixture, name)
            .get("sql")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{name} lacks sql"))
    }

    /// Strip identifier/literal quotes and collapse whitespace so Django's
    /// `str(query)` rendering compares against builder text.
    fn squish(sql: &str) -> String {
        sql.replace(['"', '\''], "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Replace every `= <rhs>` comparison target — `:placeholder`,
    /// bare/quoted live literal, or `(correlated)` — with `= ?`, so live
    /// values compare against symbolic placeholders.
    fn skeleton(sql: &str) -> String {
        let chars: Vec<char> = sql.chars().collect();
        let mut out = String::with_capacity(sql.len());
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '=' && chars.get(i + 1) == Some(&' ') {
                out.push_str("= ?");
                i += 2;
                if chars.get(i) == Some(&'(') {
                    let mut depth = 0;
                    while i < chars.len() {
                        if chars[i] == '(' {
                            depth += 1;
                        } else if chars[i] == ')' {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        i += 1;
                    }
                } else {
                    while i < chars.len()
                        && (chars[i].is_ascii_alphanumeric()
                            || matches!(chars[i], '_' | ':' | '-' | '.'))
                    {
                        i += 1;
                    }
                }
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out
    }

    /// Map the readable subquery aliases to Django's join-order artifacts.
    fn django_aliases(sql: &str) -> String {
        sql.replace("sub_issue.", "U0.")
            .replace(" sub_issue ", " U0 ")
            .replace("sub_state.", "U1.")
            .replace(" sub_state ", " U1 ")
            .replace("sub_project.", "U2.")
            .replace(" sub_project ", " U2 ")
            .replace("cycle_issue.", "U0.")
            .replace(" cycle_issue ", " U0 ")
            .replace("issue_link.", "U0.")
            .replace(" issue_link ", " U0 ")
            .replace("file_asset.", "U0.")
            .replace(" file_asset ", " U0 ")
            .replace("parent_issue.", "T5.")
            .replace(" parent_issue ", " T5 ")
    }

    /// The outer `WHERE (...)` clause of a recorded statement, without
    /// the trailing `ORDER BY` (the last `WHERE`: annotation subqueries
    /// carry their own earlier ones).
    fn fixture_where(sql: &str) -> &str {
        let from = sql.rfind("WHERE").expect("recorded WHERE");
        let tail = sql[from..]
            .find("ORDER BY")
            .map(|i| from + i)
            .unwrap_or(sql.len());
        sql[from..tail].trim()
    }

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 4).expect("valid date")
    }

    const STATE_ID: &str = "97b22834-b823-4109-a527-b39aa310ceae";
    const LABEL_ID: &str = "c833c492-fa3d-49f5-85a4-0ef810642731";
    const PARENT_ID: &str = "1431bba5-e025-4350-98c9-43d6cb58863f";
    const ASSIGNEE_ID: &str = "79c81d76-5a93-4d3d-894d-5935576834b6";

    /// The fixture world's states/labels (`Todo`; `bug` + `feature`).
    fn world_states() -> Vec<(String, String)> {
        vec![(STATE_ID.to_owned(), "Todo".to_owned())]
    }

    fn world_labels() -> Vec<(String, String)> {
        vec![
            (LABEL_ID.to_owned(), "bug".to_owned()),
            (
                "00000000-0000-0000-0000-000000000000".to_owned(),
                "feature".to_owned(),
            ),
        ]
    }

    /// `iexact` parent resolution over the fixture world (`CT00003-1`).
    fn world_parent(identifier: &str, sequence: i64) -> Option<String> {
        if identifier.eq_ignore_ascii_case("CT00003") && sequence == 1 {
            Some(PARENT_ID.to_owned())
        } else {
            None
        }
    }

    fn compile(pairs: &[(&str, &str)]) -> Result<IssueFilter, WorkItemFilterError> {
        compile_work_item_filters(
            &params(pairs),
            &world_states(),
            &world_labels(),
            world_parent,
            today(),
        )
    }

    fn sample<'a>(fixture: &'a Value, name: &str) -> &'a Value {
        &unit(fixture, "work_item_list_filters")["samples"][name]
    }

    // -- base scope replay -------------------------------------------------

    #[test]
    fn list_queryset_where_matches_fixture() {
        let fixture = fixture();
        let recorded = fixture_where(unit_sql(&fixture, "list_get_queryset"));
        // Strip the single wrapping paren pair Django renders.
        let recorded = recorded.trim_start_matches("WHERE (").trim_end_matches(')');
        assert_eq!(
            skeleton(&squish(recorded)),
            skeleton(&squish(&list_queryset_where()))
        );
    }

    #[test]
    fn workspace_queryset_where_matches_fixture() {
        let fixture = fixture();
        let recorded = fixture_where(unit_sql(&fixture, "workspace_get_queryset"));
        let recorded = recorded.trim_start_matches("WHERE (").trim_end_matches(')');
        assert_eq!(
            skeleton(&squish(recorded)),
            skeleton(&squish(&workspace_queryset_where()))
        );
    }

    #[test]
    fn base_joins_match_fixture_from_clause() {
        let fixture = fixture();
        let sql = unit_sql(&fixture, "list_get_queryset");
        let from = sql.rfind("FROM \"issues\"").expect("FROM");
        let end = sql.rfind("WHERE").expect("WHERE");
        let recorded = &sql[from + 4..end];
        // Drop the base table; compare the four joins.
        let recorded = recorded.replacen("\"issues\"", "", 1);
        assert_eq!(
            squish(&recorded),
            squish(&django_aliases(&base_joins_sql()))
        );
    }

    #[test]
    fn sub_issues_count_matches_fixture() {
        let fixture = fixture();
        let sql = unit_sql(&fixture, "list_get_queryset");
        let start = sql.find("(SELECT Count").expect("subquery");
        let end =
            sql.find("AS \"sub_issues_count\"").expect("alias") + "AS \"sub_issues_count\"".len();
        assert_eq!(
            squish(&sql[start..end]),
            squish(&django_aliases(&sub_issues_count_sql()))
        );
        // Capital-C `Count` (ported quirk 3).
        assert!(sub_issues_count_sql().contains("Count("));
    }

    #[test]
    fn detail_queryset_is_the_list_chain() {
        assert_eq!(detail_queryset_where(), list_queryset_where());
        assert_eq!(unit_row_count(&fixture(), "detail_get_queryset"), 7);
    }

    fn unit_row_count(fixture: &Value, name: &str) -> usize {
        unit(fixture, name)
            .get("row_count")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .or_else(|| {
                unit(fixture, name)
                    .get("rows")
                    .and_then(Value::as_array)
                    .map(Vec::len)
            })
            .unwrap_or_else(|| panic!("{name} lacks rows"))
    }

    #[test]
    fn list_rows_shape_is_seven_issues() {
        let fixture = fixture();
        let rows = unit(&fixture, "list_get_queryset")["rows"]
            .as_array()
            .expect("rows");
        assert_eq!(rows.len(), 7);
        assert_eq!(
            rows[0]["name"],
            Value::String("Bad-default probe".to_owned())
        );
        // Full row replay needs a live seed; the contract gate (PIDASHCONV-76)
        // owns live verification. Here: key sets are stable.
        for row in rows {
            for key in ["id", "sequence_id", "name", "priority", "sub_issues_count"] {
                assert!(row.get(key).is_some(), "row lacks {key}");
            }
        }
    }

    // -- annotation replay --------------------------------------------------

    #[test]
    fn link_and_attachment_counts_match_fixture_slice() {
        let fixture = fixture();
        let slice = unit(&fixture, "ordering")["variants"]["default_-created_at"]["order_by_sql"]
            .as_str()
            .expect("slice");
        let slice = squish(slice);
        assert!(
            slice.contains(&squish(&django_aliases(&link_count_sql()))),
            "link_count not in slice"
        );
        assert!(
            slice.contains(&squish(&django_aliases(&attachment_count_sql()))),
            "attachment_count not in slice"
        );
    }

    #[test]
    fn cycle_id_tail_matches_fixture_and_doubles_deleted_at() {
        let fixture = fixture();
        let slice = unit(&fixture, "ordering")["variants"]["default_-created_at"]["order_by_sql"]
            .as_str()
            .expect("slice");
        assert!(
            squish(slice).contains(&squish(
                "ORDER BY U0.\"created_at\" DESC LIMIT 1) AS \"cycle_id\""
            )),
            "cycle_id tail not in slice"
        );
        // Ported quirk 2: manager scope + explicit filter, no dedupe
        // (verified by rendering through Django 4.2.30).
        let sql = cycle_id_sql();
        assert_eq!(sql.matches("cycle_issue.\"deleted_at\" IS NULL").count(), 2);
        assert!(sql.contains("ORDER BY cycle_issue.\"created_at\" DESC LIMIT 1) AS \"cycle_id\""));
    }

    // -- ordering replay ----------------------------------------------------

    fn order_slice<'a>(fixture: &'a Value, variant: &str) -> &'a str {
        unit(fixture, "ordering")["variants"][variant]["order_by_sql"]
            .as_str()
            .unwrap_or_else(|| panic!("{variant} lacks order_by_sql"))
    }

    #[test]
    fn priority_case_matches_fixture_both_signs() {
        let fixture = fixture();
        let forward: Vec<&str> = PRIORITY_ORDER.to_vec();
        let backward: Vec<&str> = PRIORITY_ORDER.iter().rev().copied().collect();
        assert!(squish(order_slice(&fixture, "priority"))
            .contains(&squish(&priority_annotation_sql(&forward))));
        assert!(squish(order_slice(&fixture, "-priority"))
            .contains(&squish(&priority_annotation_sql(&backward))));
        // Both branches sort the CASE ascending (reversed list, not DESC).
        for spec in [order_spec("priority"), order_spec("-priority")] {
            match spec {
                OrderSpec::Ordered {
                    order_by_sql,
                    requires_grouping,
                    ..
                } => {
                    assert!(order_by_sql.ends_with("END ASC"));
                    assert!(!requires_grouping);
                    assert!(
                        !order_by_sql.contains("created_at"),
                        "no tiebreak (quirk 7)"
                    );
                }
                OrderSpec::Unsupported => panic!("priority must render"),
            }
        }
        // The reversed list sorts `none` first (fixture `-priority` ids).
        let ids = unit(&fixture, "ordering")["variants"]["-priority"]["ids"]
            .as_array()
            .expect("ids");
        assert_eq!(ids.len(), 7);
    }

    #[test]
    fn state_case_matches_fixture() {
        let fixture = fixture();
        let forward: Vec<&str> = STATE_ORDER.to_vec();
        assert!(squish(order_slice(&fixture, "state__group"))
            .contains(&squish(&state_order_annotation_sql(&forward))));
        assert!(state_order_annotation_sql(&forward).contains("ELSE 7 END"));
        // The v1 `[::-1]` alternative is live (unlike the app port).
        let backward: Vec<&str> = STATE_ORDER.iter().rev().copied().collect();
        match order_spec("-state__group") {
            OrderSpec::Ordered { annotation_sql, .. } => {
                assert_eq!(annotation_sql, Some(state_order_annotation_sql(&backward)));
            }
            OrderSpec::Unsupported => panic!("-state__group must render"),
        }
        // Ported bug 1: `state__name` orders by group, never by name.
        assert_eq!(order_spec("state__name"), order_spec("state__group"));
        assert_eq!(order_spec("-state__name"), order_spec("-state__group"));
    }

    #[test]
    fn max_ordering_matches_fixture() {
        let fixture = fixture();
        assert!(squish(order_slice(&fixture, "labels__name"))
            .contains(&squish("MAX(\"labels\".\"name\") AS \"max_values\"")));
        match order_spec("labels__name") {
            OrderSpec::Ordered {
                annotation_sql,
                joins_sql,
                order_by_sql,
                requires_grouping,
            } => {
                assert_eq!(
                    annotation_sql.as_deref(),
                    Some("MAX(\"order_labels\".\"name\") AS \"max_values\"")
                );
                assert!(joins_sql.starts_with("LEFT OUTER JOIN \"issue_labels\""));
                assert!(joins_sql.contains("LEFT OUTER JOIN \"labels\""));
                assert_eq!(order_by_sql, "MAX(\"order_labels\".\"name\") ASC");
                assert!(requires_grouping);
            }
            OrderSpec::Unsupported => panic!("labels__name must render"),
        }
        match order_spec("-assignees__first_name") {
            OrderSpec::Ordered {
                order_by_sql,
                joins_sql,
                requires_grouping,
                ..
            } => {
                assert_eq!(order_by_sql, "MAX(\"order_users\".\"first_name\") DESC");
                assert!(joins_sql.contains("LEFT OUTER JOIN \"users\""));
                assert!(requires_grouping);
            }
            OrderSpec::Unsupported => panic!("assignees must render"),
        }
    }

    #[test]
    fn default_ordering_covers_param_shapes() {
        let ordered = |param: &str| match order_spec(param) {
            OrderSpec::Ordered {
                order_by_sql,
                annotation_sql,
                requires_grouping,
                ..
            } => {
                assert_eq!(annotation_sql, None);
                assert!(!requires_grouping);
                order_by_sql
            }
            OrderSpec::Unsupported => panic!("{param} must render"),
        };
        assert_eq!(ordered("-created_at"), "\"issues\".\"created_at\" DESC");
        assert_eq!(ordered("created_at"), "\"issues\".\"created_at\" ASC");
        assert_eq!(ordered("name"), "\"issues\".\"name\" ASC");
        assert_eq!(ordered("pk"), "\"issues\".\"id\" ASC");
        assert_eq!(ordered("state"), "\"issues\".\"state_id\" ASC");
        assert_eq!(ordered("-sequence_id"), "\"issues\".\"sequence_id\" DESC");
        assert_eq!(ordered("cycle_id"), "\"cycle_id\" ASC");
        assert_eq!(ordered("-link_count"), "\"link_count\" DESC");
        assert_eq!(ordered("state__sequence"), "\"states\".\"sequence\" ASC");
        assert_eq!(ordered("-project__name"), "\"projects\".\"name\" DESC");
        assert_eq!(ordered("?"), "RANDOM()");
        // Unknown bare names splice qualified (the database 500s, as
        // Django's FieldError does) without injection surface.
        assert_eq!(ordered("nope"), "\"issues\".\"nope\" ASC");
        // Multi-level paths needing new joins are unsupported (quirk 12).
        assert_eq!(order_spec("labels__id"), OrderSpec::Unsupported);
        assert_eq!(
            order_spec("project__workspace__slug"),
            OrderSpec::Unsupported
        );
        assert_eq!(order_spec("--created_at"), OrderSpec::Unsupported);
        assert_eq!(order_spec(""), OrderSpec::Unsupported);
    }

    // -- filter compiler replay (all 17 F18-06 samples) ------------------------

    #[test]
    fn empty_params_yield_empty_filter() {
        let filters = compile(&[]).expect("ok");
        assert!(filters.is_empty());
        assert!(sample(&fixture(), "empty")["ok"]
            .as_bool()
            .expect("ok flag"));
        assert_eq!(filter_where_sql(&filters), "");
        assert_eq!(filter_joins_sql(&filters), "");
    }

    fn uuids(value: &FilterValue) -> Vec<String> {
        match value {
            FilterValue::Uuids(ids) => ids.iter().map(ToString::to_string).collect(),
            other => panic!("expected Uuids, got {other:?}"),
        }
    }

    fn strings(value: &FilterValue) -> Vec<String> {
        match value {
            FilterValue::Strings(items) => items.clone(),
            other => panic!("expected Strings, got {other:?}"),
        }
    }

    #[test]
    fn state_uuid_and_name_resolve_to_state_in() {
        let expected = ["97b22834-b823-4109-a527-b39aa310ceae".to_owned()];
        for pair in [("state", STATE_ID), ("state", "Todo"), ("state", "todo")] {
            let filters = compile(&[pair]).expect("ok");
            assert_eq!(
                uuids(filters.get("state__in").expect("state__in")),
                expected
            );
        }
        let fixture = fixture();
        for name in ["state_uuid", "state_name"] {
            let sample = sample(&fixture, name);
            assert!(sample["ok"].as_bool().expect("ok"));
            assert!(sample["filters"]["state__in"]
                .as_str()
                .expect("repr")
                .contains(STATE_ID));
        }
    }

    #[test]
    fn unknown_state_name_errors_exactly() {
        let err = compile(&[("state", "Nope")]).expect_err("must fail");
        assert_eq!(
            err.to_string(),
            "Unknown state name(s): Nope. Valid states: Todo."
        );
        assert_eq!(
            sample(&fixture(), "state_unknown")["error"]
                .as_str()
                .expect("error"),
            "Unknown state name(s): Nope. Valid states: Todo."
        );
        assert_eq!(
            err.body(),
            "{\"error\":\"Unknown state name(s): Nope. Valid states: Todo.\"}"
        );
    }

    #[test]
    fn state_groups_validate_and_compile() {
        let filters = compile(&[("state_group", "Unstarted,started")]).expect("ok");
        assert_eq!(
            strings(filters.get("state__group__in").expect("groups")),
            ["unstarted".to_owned(), "started".to_owned()]
        );
        let fixture = fixture();
        let sample = sample(&fixture, "state_group");
        assert!(sample["ok"].as_bool().expect("ok"));
        let repr = sample["filters"]["state__group__in"]
            .as_str()
            .expect("repr");
        assert!(repr.find("unstarted").expect("u") < repr.find("started").expect("s"));
        // The shared order tuple is the fixture's group order, in order.
        let order: Vec<&str> = unit(&fixture, "work_item_list_filters")["state_group_order"]
            .as_array()
            .expect("order")
            .iter()
            .map(|v| v.as_str().expect("group"))
            .collect();
        assert_eq!(order, STATE_ORDER);
    }

    #[test]
    fn bad_state_group_errors_exactly() {
        let err = compile(&[("state_group", "bogus")]).expect_err("must fail");
        assert_eq!(
            err.to_string(),
            "Unknown state group(s): bogus. Valid groups: backlog, unstarted, started, review, test, completed, cancelled."
        );
        assert_eq!(
            sample(&fixture(), "state_group_bad")["error"]
                .as_str()
                .expect("error"),
            err.to_string()
        );
    }

    #[test]
    fn priorities_validate_and_compile() {
        let filters = compile(&[("priority", "HIGH,low")]).expect("ok");
        assert_eq!(
            strings(filters.get("priority__in").expect("priorities")),
            ["high".to_owned(), "low".to_owned()]
        );
        assert_eq!(VALID_PRIORITIES, PRIORITY_ORDER);
        let fixture = fixture();
        assert!(sample(&fixture, "priority")["ok"].as_bool().expect("ok"));
    }

    #[test]
    fn bad_priority_errors_exactly() {
        let err = compile(&[("priority", "critical")]).expect_err("must fail");
        assert_eq!(
            err.to_string(),
            "Unknown priority value(s): critical. Valid priorities: urgent, high, medium, low, none."
        );
        assert_eq!(
            sample(&fixture(), "priority_bad")["error"]
                .as_str()
                .expect("error"),
            err.to_string()
        );
    }

    #[test]
    fn parent_null_spells_top_level_only() {
        for raw in ["null", "none", "NULL", "None"] {
            let filters = compile(&[("parent", raw)]).expect("ok");
            assert_eq!(
                filters.get("parent__isnull"),
                Some(&FilterValue::Flag(true))
            );
            assert_eq!(filters.get("parent__in"), None);
        }
        let fixture = fixture();
        let sample = sample(&fixture, "parent_null");
        assert!(sample["ok"].as_bool().expect("ok"));
        assert_eq!(sample["filters"]["parent__isnull"].as_str(), Some("True"));
    }

    #[test]
    fn parent_mixed_null_and_specific_errors() {
        let err = compile(&[("parent", "null,CT00003-1")]).expect_err("must fail");
        assert_eq!(
            err.to_string(),
            "parent=null cannot be combined with specific parent issues."
        );
        assert_eq!(
            sample(&fixture(), "parent_mixed")["error"]
                .as_str()
                .expect("error"),
            err.to_string()
        );
    }

    #[test]
    fn parent_token_edges() {
        // UUIDs (any case/braces) pass through.
        let filters = compile(&[("parent", "97B22834-B823-4109-A527-B39AA310CEAE")]).expect("ok");
        assert_eq!(
            uuids(filters.get("parent__in").expect("parents")),
            ["97b22834-b823-4109-a527-b39aa310ceae".to_owned()]
        );
        // Identifiers resolve case-insensitively (iexact).
        let filters = compile(&[("parent", "ct00003-1")]).expect("ok");
        assert_eq!(
            uuids(filters.get("parent__in").expect("parents")),
            [PARENT_ID.to_owned()]
        );
        let fixture = fixture();
        assert!(sample(&fixture, "parent_ident")["ok"]
            .as_bool()
            .expect("ok"));
        assert!(sample(&fixture, "parent_ident")["filters"]["parent__in"]
            .as_str()
            .expect("repr")
            .contains(PARENT_ID));
        // Malformed tokens.
        for raw in ["zzz", "5", "-5", "PROJ-", "PROJ-12X", "PROJ-1-2X"] {
            let err = compile(&[("parent", raw)]).expect_err(raw);
            assert!(
                err.to_string()
                    .starts_with(&format!("Invalid parent '{raw}'")),
                "{raw}"
            );
        }
        assert_eq!(
            sample(&fixture, "parent_bad")["error"]
                .as_str()
                .expect("error"),
            "Invalid parent 'zzz': expected an issue UUID, an identifier like PROJ-123, or null."
        );
        // Unknown identifiers.
        let err = compile(&[("parent", "ZZZ-999")]).expect_err("must fail");
        assert_eq!(err.to_string(), "Unknown parent issue 'ZZZ-999'.");
        assert_eq!(
            sample(&fixture, "parent_unknown_ident")["error"]
                .as_str()
                .expect("error"),
            err.to_string()
        );
        // Overflowing magnitudes are valid-but-unknown (Postgres widens).
        let err = compile(&[("parent", "CT00003-99999999999999999999999")]).expect_err("overflow");
        assert_eq!(
            err.to_string(),
            "Unknown parent issue 'CT00003-99999999999999999999999'."
        );
    }

    #[test]
    fn labels_resolve_names_and_compile_through_model() {
        let filters = compile(&[("labels", "Bug")]).expect("ok");
        assert_eq!(
            strings(filters.get("label_issue__label_id__in").expect("ids")),
            [LABEL_ID.to_owned()]
        );
        assert_eq!(
            filters.get("label_issue__deleted_at__isnull"),
            Some(&FilterValue::Flag(true))
        );
        let fixture = fixture();
        let sample = sample(&fixture, "labels_name");
        assert!(sample["ok"].as_bool().expect("ok"));
        assert!(sample["filters"]["label_issue__label_id__in"]
            .as_str()
            .expect("repr")
            .contains(LABEL_ID));
        assert_eq!(
            sample["filters"]["label_issue__deleted_at__isnull"].as_str(),
            Some("True")
        );
    }

    #[test]
    fn unknown_label_errors_exactly() {
        let err = compile(&[("labels", "nope")]).expect_err("must fail");
        assert_eq!(
            err.to_string(),
            "Unknown label name(s): nope. Valid labels: bug, feature."
        );
        assert_eq!(
            sample(&fixture(), "labels_unknown")["error"]
                .as_str()
                .expect("error"),
            err.to_string()
        );
    }

    #[test]
    fn assignees_validate_uuids_and_compile_through_model() {
        let filters = compile(&[("assignees", ASSIGNEE_ID)]).expect("ok");
        assert_eq!(
            strings(filters.get("issue_assignee__assignee_id__in").expect("ids")),
            [ASSIGNEE_ID.to_owned()]
        );
        assert_eq!(
            filters.get("issue_assignee__deleted_at__isnull"),
            Some(&FilterValue::Flag(true))
        );
        let fixture = fixture();
        assert!(sample(&fixture, "assignees_ok")["ok"]
            .as_bool()
            .expect("ok"));
        let err = compile(&[("assignees", "zzz")]).expect_err("must fail");
        assert_eq!(
            err.to_string(),
            "Invalid assignee id(s): zzz. Expected user UUIDs."
        );
        assert_eq!(
            sample(&fixture, "assignees_bad")["error"]
                .as_str()
                .expect("error"),
            err.to_string()
        );
    }

    #[test]
    fn validation_order_is_state_group_priority_parent_labels_assignees() {
        // The first failure in Python validation order wins.
        let err = compile(&[("state", "Nope"), ("priority", "critical")]).expect_err("state first");
        assert!(err.to_string().starts_with("Unknown state name(s)"));
        let err = compile(&[("priority", "critical"), ("state_group", "bogus")])
            .expect_err("group first");
        assert!(err.to_string().starts_with("Unknown state group(s)"));
        let err = compile(&[("assignees", "zzz"), ("labels", "nope")]).expect_err("labels first");
        assert!(err.to_string().starts_with("Unknown label name(s)"));
    }

    #[test]
    fn duplicate_names_resolve_repeatedly() {
        let states = vec![
            (
                "11111111-1111-1111-1111-111111111111".to_owned(),
                "Todo".to_owned(),
            ),
            (
                "22222222-2222-2222-2222-222222222222".to_owned(),
                "TODO".to_owned(),
            ),
        ];
        let ids = resolve_states(&["todo".to_owned()], &states).expect("ok");
        assert_eq!(ids.len(), 2);
        // Unknown lists repeat duplicates in input order.
        let err = resolve_states(&["a".to_owned(), "a".to_owned()], &states).expect_err("unknown");
        assert!(err.to_string().starts_with("Unknown state name(s): a, a."));
    }

    #[test]
    fn labels_none_fallback_and_states_no_fallback() {
        // Ported quirk 9: empty label scope renders `(none)`.
        let err = resolve_labels(&["x".to_owned()], &[]).expect_err("unknown");
        assert_eq!(
            err.to_string(),
            "Unknown label name(s): x. Valid labels: (none)."
        );
        // States have no fallback.
        let err = resolve_states(&["x".to_owned()], &[]).expect_err("unknown");
        assert_eq!(err.to_string(), "Unknown state name(s): x. Valid states: .");
    }

    #[test]
    fn filter_where_renders_q_sorted() {
        let filters = compile(&[
            ("state", "Todo"),
            ("state_group", "started"),
            ("priority", "high"),
            ("parent", "null"),
            ("labels", "bug"),
            ("assignees", ASSIGNEE_ID),
        ])
        .expect("ok");
        // `Q` key-sorted, not insertion-ordered (verified via Django render).
        let keys: Vec<&str> = filters
            .predicates()
            .iter()
            .map(|(key, _)| key.as_str())
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        let rendered = filter_where_sql(&filters);
        let mut last = 0;
        for fragment in [
            "filter_assignees.\"assignee_id\" IN (:assignee_ids)",
            "filter_assignees.\"deleted_at\" IS NULL",
            "filter_labels.\"deleted_at\" IS NULL",
            "filter_labels.\"label_id\" IN (:label_ids)",
            "\"issues\".\"parent_id\" IS NULL",
            "\"issues\".\"priority\" IN (:priorities)",
            "\"states\".\"group\" IN (:state_groups)",
            "\"states\".\"id\" IN (:states)",
        ] {
            let at = rendered
                .find(fragment)
                .unwrap_or_else(|| panic!("missing {fragment}"));
            assert!(at >= last, "{fragment} out of Q-sorted order");
            last = at;
        }
        assert_eq!(keys.len(), 8);
        assert!(sorted.windows(2).all(|pair| pair[0] <= pair[1]));
        // Joins follow the same order (assignees first).
        let joins = filter_joins_sql(&filters);
        assert!(joins.find("issue_assignees").expect("a") < joins.find("issue_labels").expect("l"));
    }

    // -- paginate surface replay ------------------------------------------------

    #[test]
    fn envelope_keys_match_fixture_set() {
        let fixture = fixture();
        let recorded: Vec<&str> = unit(&fixture, "paginator")["envelope_keys"]
            .as_array()
            .expect("keys")
            .iter()
            .map(|v| v.as_str().expect("key"))
            .collect();
        let mut mine = PAGINATE_ENVELOPE_KEYS.to_vec();
        let mut theirs = recorded;
        mine.sort();
        theirs.sort();
        assert_eq!(mine, theirs);
        // Wire order is the `paginate` dict order (paginator.py:717-732).
        assert_eq!(
            PAGINATE_ENVELOPE_KEYS,
            [
                "grouped_by",
                "sub_grouped_by",
                "total_count",
                "next_cursor",
                "prev_cursor",
                "next_page_results",
                "prev_page_results",
                "count",
                "total_pages",
                "total_results",
                "extra_stats",
                "results",
            ]
        );
    }

    #[test]
    fn per_page_errors_match_fixture() {
        let fixture = fixture();
        let unit = unit(&fixture, "paginator");
        assert_eq!(
            parse_per_page(Some("lots")).expect_err("invalid").message,
            unit["per_page_invalid"]["body"]["detail"]
                .as_str()
                .expect("detail")
        );
        assert_eq!(
            parse_per_page(Some("1001")).expect_err("over max").message,
            "Invalid per_page value. Cannot exceed 1000."
        );
        assert_eq!(
            unit["per_page_over_max"]["body"]["detail"].as_str(),
            Some("Invalid per_page value. Cannot exceed 1000.")
        );
        assert_eq!(parse_per_page(None).expect("default"), 1000);
        assert_eq!(PAGINATE_DEFAULT_PER_PAGE, 1000);
        assert_eq!(PAGINATE_MAX_PER_PAGE, 1000);
    }

    #[test]
    fn cursor_surface_matches_fixture() {
        assert_eq!(CURSOR_PARAM, "cursor");
        assert_eq!(CURSOR_FORMAT, "value:offset:is_prev");
        assert_eq!(default_cursor(1000), "1000:0:0");
        assert_eq!(default_cursor(2), "2:0:0");
        // Window math is the shared kernel's; the recorded cursors pin its
        // contract (`{limit}:{page}:{is_prev}`).
        let fixture = fixture();
        let page1 = &unit(&fixture, "paginator")["page1"];
        assert_eq!(page1["next_cursor"].as_str(), Some("2:1:0"));
        assert_eq!(page1["prev_cursor"].as_str(), Some("2:-1:1"));
        assert_eq!(PAGINATOR_KERNEL, "pidash_api::paginator");
    }

    #[test]
    fn total_count_skips_distinct_without_filters() {
        let (unfiltered, distinct) = total_count_query(&IssueFilter::default());
        assert!(!distinct);
        assert_eq!(
            unfiltered,
            format!("{} AND {}", ISSUE_MANAGER_WHERE, project_scope_sql())
        );
        let filters = compile(&[("priority", "high")]).expect("ok");
        let (filtered, distinct) = total_count_query(&filters);
        assert!(distinct);
        assert!(filtered.contains("\"issues\".\"priority\" IN (:priorities)"));
    }

    // -- lookup replay ----------------------------------------------------------

    #[test]
    fn external_id_lookup_matches_fixture() {
        let fixture = fixture();
        let sql = unit_sql(&fixture, "external_id_lookup");
        let recorded = fixture_where(sql)
            .trim_start_matches("WHERE (")
            .trim_end_matches(')');
        assert_eq!(
            skeleton(&squish(recorded)),
            skeleton(&squish(&external_id_lookup_where()))
        );
        let from = sql.rfind("FROM \"issues\"").expect("FROM");
        let end = sql.find("WHERE").expect("WHERE");
        assert_eq!(
            squish(&sql[from + 4..end].replacen("\"issues\"", "", 1)),
            squish(&external_id_lookup_joins_sql())
        );
    }

    #[test]
    fn inline_lookups_match_fixture() {
        let fixture = fixture();
        for (name, built) in [
            ("detail", detail_get_lookup_where()),
            ("workspace", workspace_get_lookup_where()),
            ("patch", patch_lookup_where()),
            ("delete", delete_lookup_where()),
        ] {
            let key = match name {
                "detail" => "detail_get_lookup",
                "workspace" => "workspace_get_lookup",
                "patch" => "patch_lookup",
                "delete" => "delete_lookup",
                _ => panic!("unknown lookup {name}"),
            };
            let sql = unit_sql(&fixture, key);
            let recorded = fixture_where(sql)
                .trim_start_matches("WHERE (")
                .trim_end_matches(')');
            assert_eq!(
                skeleton(&squish(recorded)),
                skeleton(&squish(&built)),
                "{name} WHERE mismatch"
            );
        }
        assert_eq!(delete_lookup_where(), patch_lookup_where());
        assert_eq!(LOOKUP_ORDER_SQL, "\"issues\".\"created_at\" DESC");
        // Inline `.get()` joins: manager scope + workspaces (fixture order).
        let sql = unit_sql(&fixture, "detail_get_lookup");
        let from = sql.rfind("FROM \"issues\"").expect("FROM");
        let end = sql.rfind("WHERE").expect("WHERE");
        assert_eq!(
            squish(&sql[from + 4..end].replacen("\"issues\"", "", 1)),
            squish(&inline_lookup_joins_sql())
        );
    }

    #[test]
    fn put_upsert_read_path_matches_fixture() {
        let fixture = fixture();
        let sql = unit(&fixture, "put_upsert")["lookup_sql"]
            .as_str()
            .expect("lookup_sql");
        let recorded = fixture_where(sql)
            .trim_start_matches("WHERE (")
            .trim_end_matches(')');
        assert_eq!(
            skeleton(&squish(recorded)),
            skeleton(&squish(&put_upsert_lookup_where()))
        );
        assert_eq!(
            unit(&fixture, "put_upsert")["missing_keys_body"]["error"].as_str(),
            Some(PUT_MISSING_KEYS_MESSAGE)
        );
        assert_eq!(
            unit(&fixture, "put_upsert")["missing_keys_status"].as_u64(),
            Some(400)
        );
        assert_eq!(
            put_missing_keys_body(),
            "{\"error\":\"external_id and external_source are required\"}"
        );
        assert_eq!(
            PUT_REFETCH_TAIL_SQL,
            "ORDER BY \"issues\".\"created_at\" DESC LIMIT 1"
        );
        assert_eq!(
            put_upsert_lookup_joins_sql(),
            external_id_lookup_joins_sql()
        );
        assert_eq!(pk_lookup_joins_sql(), external_id_lookup_joins_sql());
    }

    // -- resolution lookup SQL ----------------------------------------------------

    #[test]
    fn states_lookup_selects_live_nontriage_in_sequence() {
        assert_eq!(
            states_lookup_sql(),
            "SELECT \"states\".\"id\", \"states\".\"name\" FROM \"states\" WHERE (\"states\".\"deleted_at\" IS NULL AND NOT (\"states\".\"group\" = 'triage') AND \"states\".\"project_id\" = :project_id) ORDER BY \"states\".\"sequence\" ASC"
        );
    }

    #[test]
    fn labels_lookup_covers_project_and_workspace_scope() {
        assert_eq!(
            labels_lookup_sql(),
            "SELECT \"labels\".\"id\", \"labels\".\"name\" FROM \"labels\" LEFT OUTER JOIN \"workspaces\" ON (\"labels\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"labels\".\"deleted_at\" IS NULL AND (\"labels\".\"project_id\" = :project_id OR (\"labels\".\"project_id\" IS NULL AND \"workspaces\".\"slug\" = :workspace_slug))) ORDER BY \"labels\".\"created_at\" DESC"
        );
    }

    #[test]
    fn parent_lookup_uses_manager_scope_and_iexact() {
        let sql = parent_lookup_sql();
        assert!(sql.starts_with("SELECT \"issues\".\"id\" FROM \"issues\" "));
        assert!(sql.contains(ISSUE_MANAGER_WHERE));
        assert!(
            sql.contains("UPPER(\"projects\".\"identifier\"::text) = UPPER(:project_identifier)")
        );
        assert!(sql.ends_with("ORDER BY \"issues\".\"created_at\" DESC LIMIT 1"));
    }

    // -- small units ---------------------------------------------------------------

    #[test]
    fn split_tokens_and_is_uuid_edges() {
        assert!(split_tokens(None).is_empty());
        assert!(split_tokens(Some("")).is_empty());
        assert!(split_tokens(Some("  ,, ")).is_empty());
        assert_eq!(
            split_tokens(Some("a,, b ,")),
            ["a".to_owned(), "b".to_owned()]
        );
        assert!(is_uuid("97b22834-b823-4109-a527-b39aa310ceae"));
        assert!(is_uuid("97B22834B8234109A527B39AA310CEAE"));
        assert!(is_uuid("{97b22834-b823-4109-a527-b39aa310ceae}"));
        assert!(is_uuid("urn:uuid:97b22834-b823-4109-a527-b39aa310ceae"));
        assert!(!is_uuid("zzz"));
        assert!(!is_uuid(""));
    }

    #[test]
    fn module_consts_match_sources() {
        assert_eq!(
            WORK_ITEM_LIST_FILTER_KEYS,
            [
                "state",
                "state_group",
                "parent",
                "labels",
                "priority",
                "assignees"
            ]
        );
        assert_eq!(
            COMPILED_PREDICATE_KEYS,
            [
                "state__in",
                "state__group__in",
                "priority__in",
                "parent__isnull",
                "parent__in",
                "label_issue__label_id__in",
                "label_issue__deleted_at__isnull",
                "issue_assignee__assignee_id__in",
                "issue_assignee__deleted_at__isnull",
            ]
        );
        const { assert!(QUERYSET_DISTINCT) };
        assert_eq!(QUERYSET_DEFAULT_ORDER, "-created_at");
        assert_eq!(LIST_DEFAULT_ORDER_PARAM, "-created_at");
        assert_eq!(SELECT_RELATED, ["project", "workspace", "state", "parent"]);
        assert_eq!(PREFETCH_RELATED, ["assignees", "labels"]);
        assert_eq!(
            LIST_ANNOTATIONS,
            [
                "sub_issues_count",
                "cycle_id",
                "link_count",
                "attachment_count"
            ]
        );
        assert_eq!(ORDERABLE_ISSUE_COLUMNS.len(), 34);
    }
}
