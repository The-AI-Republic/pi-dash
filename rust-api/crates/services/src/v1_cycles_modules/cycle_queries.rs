//! Cycle read orchestration over caller-supplied facts (D-20, stage 5).
//!
//! Ports the read-choice logic around the five cycle querysets in
//! `apps/api/pi_dash/api/views/cycle.py` (drift baseline `01a93e17`)
//! plus the `transfer_cycle_issues` decision flow
//! (`utils/cycle_transfer_issues.py:36-479`): which archived filter
//! each endpoint applies, which order default it carries, which shapes
//! need the acting user, which `cycle_view` arm a list `GET` takes —
//! and, for transfers, the guard checks, the estimate branch and the
//! `progress_snapshot` assembly. The SQL itself lives in the db sibling
//! (`pidash_db::v1_cycles_modules::cycle_queries`); this module decides
//! *which* builder the handler calls and with what inputs, composed over
//! facts the handler already loaded (URL kwargs, query params, actor,
//! DB rows).
//!
//! Fixture oracle: FX-CYCMOD-04 (same `cycle.sql` Q1-Q6 the db sibling
//! pins). The tests below replay the endpoint → shape decision table,
//! the `cycle_view` parse and the transfer branch table — not the SQL.
//!
//! # The two orderings
//!
//! Queryset chains read `.order_by(self.kwargs.get("order_by",
//! "-created_at"))` (`cycle.py:165,446,722,835,1047`) — a URL-kwargs
//! passthrough defaulting to descending. The Q4 `GET` inline chain
//! instead reads `request.GET.get("order_by", "created_at")` (`:861`)
//! — a real query param defaulting to **ascending**. [`parse_order`]
//! serves both call sites with their own default; the leading `-`
//! selects descending exactly like Django.
//!
//! # Which shape serves which method
//!
//! * List `GET` (`:190-281`) → Q1 live ([`ReadShape::CycleList`]) plus
//!   the [`parse_cycle_view`] arm; `cycle_view=current` returns a bare
//!   list instead of the paginated envelope (ported bug —
//!   [`cycle_view_is_paginated`]).
//! * Detail `GET` (`:462-476`) → Q2 ([`ReadShape::CycleDetail`]).
//! * Archived-list `GET` (`:741-751`) → Q3
//!   ([`ReadShape::ArchivedCycleList`]).
//! * Issue-list `GET` (`:854-901`) → the Q4 inline shape
//!   ([`ReadShape::CycleIssueListGet`]); the issue-list `POST` re-read
//!   (`:1008`) serves the Q4 *queryset* shape instead
//!   ([`ReadShape::CycleIssueQueryset`]).
//! * Issue-detail `GET` (`:1063-1076`) and `DELETE` (`:1086-1114`) → the
//!   Q5 `.get()` lookup ([`ReadShape::CycleIssueDetail`]); the Q5
//!   `get_queryset` (`:1027-1049`) is dead on the wire. Unlike modules,
//!   the detail route wires `get` + `delete`
//!   ([`CYCLE_ISSUE_DETAIL_ROUTED_METHODS`]).
//! * Transfer `POST` (`:1167-1210`) → the transfer flow below; its view
//!   pre-checks (`new_cycle_id` presence at `:1173-1179`, old-cycle
//!   completion at `:1187-1191`) are handler-owned.
//!
//! Direct `.get()` call sites (detail `PATCH` `:500`, detail `DELETE`
//! `:577`, archive `POST` `:769`, unarchive `DELETE` `:800`, issue-list
//! `POST` cycle load `:934`) bypass every queryset — plain pk lookups
//! with the model's default-manager scope. They are handler-owned and
//! have no builder here; they are listed so the mapping reads total.
//!
//! # Transfer flow (`transfer_cycle_issues`, `:36-479`)
//!
//! [`check_new_cycle`] ports the new-cycle guard (`:59-66`, including
//! the missing-`None`-check 500 — ported bug T1 below);
//! [`TransferBlock::SourceCycleMissing`] ports the source-missing 400
//! (`:145-149`); [`build_progress_snapshot`] assembles the
//! `progress_snapshot` dict (`:411-432`) from caller-loaded rows; and
//! [`transfer_move_entry`] shapes one move triple (`:450-456`). The DB
//! reads are the db sibling's `TRANSFER_*` statements; the snapshot
//! save (`:433`), the `bulk_update` (`:459`) and the `issue_activity`
//! call (`:462-478`) are handler/tasks-owned (PIDASHCONV-362 /
//! PIDASHCONV-310); `burndown_plot` outputs arrive as caller-supplied
//! facts (the D-27 `burndown_*_sql` precedent in
//! `pidash_api::app_cycles::handlers_archive`).
//!
//! Ported transfer bugs:
//!
//! * T1. An unknown `new_cycle_id` raises `AttributeError` (500):
//!   `new_cycle.end_date` at `:62` has no `None` check, unlike the
//!   source lookup at `:145`. [`TransferBlock::NewCycleMissing`] maps to
//!   the generic 500 path, never a 400.
//! * T2. `cycle_view=current` returns a bare list, not the paginated
//!   envelope (`cycle.py:201-210`).
//!
//! Out of scope (sibling D-20 issues): envelopes and serializer field
//! selection (handlers, PIDASHCONV-362), `ProjectEntityPermission` gates
//! (PIDASHCONV-309), activity enqueues (PIDASHCONV-310), prefetch round
//! trips (handlers).

use pidash_db::v1_cycles_modules::cycle_queries::{ArchivedFilter, CycleView, OrderBy};

/// A cycle read shape: which db builder the handler calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadShape {
    /// Q1 list (`cycle.py:89-167` + `:197`, `:198-270`).
    CycleList,
    /// Q2 detail (`cycle.py:370-448` + `:469`).
    CycleDetail,
    /// Q3 archived list (`cycle.py:622-724`).
    ArchivedCycleList,
    /// Q4/Q5 `get_queryset` (`cycle.py:815-837` / `:1027-1049`).
    CycleIssueQueryset,
    /// Q4 `GET` inline queryset (`cycle.py:862-895`).
    CycleIssueListGet,
    /// Q5 `GET`/`DELETE` `.get()` lookup (`cycle.py:1069-1074` /
    /// `:1092-1097`).
    CycleIssueDetail,
}

/// HTTP methods the `cycle-issues-detail` route actually wires
/// (`api/urls/cycle.py:38-42`, `http_method_names=["get", "delete"]`) —
/// the reverse of the module asymmetry, where the detail `GET` exists
/// but is unrouted.
pub const CYCLE_ISSUE_DETAIL_ROUTED_METHODS: &[&str] = &["get", "delete"];

/// Parse one `.order_by(...)` / `request.GET.get("order_by", ...)`
/// argument into the [`OrderBy`] the db builder quotes.
///
/// `raw` is the kwarg (`None` = key absent → `default`) or the query
/// param (`None` = param absent → `default`); `default` is `"-created_at"`
/// for the queryset chains and `"created_at"` for the Q4 GET chain.
/// A leading `-` selects descending, exactly like Django; the column
/// text (including an empty or unknown column) passes through untouched
/// and fails at the database like Django's `FieldError`-at-evaluation.
pub fn parse_order(raw: Option<&str>, default: &str) -> OrderBy {
    let text = raw.unwrap_or(default);
    match text.strip_prefix('-') {
        Some(column) => OrderBy::new(column, true),
        None => OrderBy::new(text, false),
    }
}

/// The queryset-chain order default (`cycle.py:165,446,722,835,1047`).
pub const QUERYSET_ORDER_DEFAULT: &str = "-created_at";

/// The Q4 `GET` order default (`cycle.py:861`) — ascending, unlike
/// every queryset chain.
pub const ISSUE_GET_ORDER_DEFAULT: &str = "created_at";

/// The `cycle_view` query-param default (`cycle.py:198`).
pub const CYCLE_VIEW_DEFAULT: &str = "all";

/// Parse the `cycle_view` query param (`cycle.py:198`) into the
/// [`CycleView`] the db builder compiles. `None` (param absent) is
/// `"all"`; matching is exact and lowercase — anything unrecognized
/// falls through the view's `if` chain to the plain unfiltered list,
/// i.e. [`CycleView::All`].
pub fn parse_cycle_view(raw: Option<&str>) -> CycleView {
    match raw.unwrap_or(CYCLE_VIEW_DEFAULT) {
        "current" => CycleView::Current,
        "upcoming" => CycleView::Upcoming,
        "completed" => CycleView::Completed,
        "draft" => CycleView::Draft,
        "incomplete" => CycleView::Incomplete,
        _ => CycleView::All,
    }
}

/// Whether the `cycle_view` arm paginates (`cycle.py:201-281`): every
/// arm paginates except `current`, which returns a bare serialized list
/// (ported bug T2).
pub fn cycle_view_is_paginated(view: CycleView) -> bool {
    !matches!(view, CycleView::Current)
}

/// The `archived_at` predicate a read shape applies
/// (`cycle.py:197,469,630`).
pub fn shape_archived_filter(shape: ReadShape) -> ArchivedFilter {
    match shape {
        ReadShape::CycleList | ReadShape::CycleDetail => ArchivedFilter::Live,
        ReadShape::ArchivedCycleList => ArchivedFilter::Archived,
        ReadShape::CycleIssueQueryset
        | ReadShape::CycleIssueListGet
        | ReadShape::CycleIssueDetail => ArchivedFilter::Any,
    }
}

/// Whether the shape joins `project_members` on the acting user, i.e.
/// the handler must supply it (`cycle.py:93-96,374-377,626-629,825-828,
/// 1037-1040`). Every queryset chain does — including Q1/Q2/Q3 (the
/// asymmetry vs the module reads); only the Q4 GET shape and the Q5
/// lookup skip the member join.
pub fn shape_needs_member(shape: ReadShape) -> bool {
    !matches!(
        shape,
        ReadShape::CycleIssueListGet | ReadShape::CycleIssueDetail
    )
}

/// Whether the shape selects `DISTINCT`: every queryset chain
/// (`cycle.py:166,447,723,836,1048`); the Q4 GET shape and the Q5
/// lookup do not.
pub fn shape_is_distinct(shape: ReadShape) -> bool {
    !matches!(
        shape,
        ReadShape::CycleIssueListGet | ReadShape::CycleIssueDetail
    )
}

/// The order default a shape carries: descending `created_at` for every
/// queryset chain and the Q5 lookup's `Meta.ordering`, ascending
/// `created_at` for the Q4 GET chain.
pub fn shape_order_default(shape: ReadShape) -> &'static str {
    match shape {
        ReadShape::CycleIssueListGet => ISSUE_GET_ORDER_DEFAULT,
        _ => QUERYSET_ORDER_DEFAULT,
    }
}

// ---------------------------------------------------------------------------
// Transfer orchestration
// ---------------------------------------------------------------------------

/// `transfer_cycle_issues` failure when the target cycle already ended
/// (transfer file `:63-66`).
pub const TRANSFER_NEW_CYCLE_COMPLETED_ERROR: &str =
    "The cycle where the issues are transferred is already completed";

/// `transfer_cycle_issues` failure when the source cycle has no row
/// (transfer file `:146-149`).
pub const TRANSFER_SOURCE_CYCLE_NOT_FOUND_ERROR: &str = "Source cycle not found";

/// What blocks a transfer before any distribution loads, in check order
/// (transfer file `:62-66`, `:145-149`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferBlock {
    /// No `Cycle` row for `new_cycle_id`: `new_cycle.end_date` raises
    /// `AttributeError` → the generic 500 path (ported bug T1 — there
    /// is no `None` check, unlike the source lookup).
    NewCycleMissing,
    /// The target cycle already ended → 400
    /// ([`TRANSFER_NEW_CYCLE_COMPLETED_ERROR`]).
    NewCycleCompleted,
    /// No `Cycle` row for the source `cycle_id` → 400
    /// ([`TRANSFER_SOURCE_CYCLE_NOT_FOUND_ERROR`]).
    SourceCycleMissing,
}

impl TransferBlock {
    /// The HTTP status the handler renders (`:62-66` 500-via-`base.py`,
    /// `:63-66`/`:146-149` 400).
    pub fn status_code(self) -> u16 {
        match self {
            TransferBlock::NewCycleMissing => 500,
            TransferBlock::NewCycleCompleted | TransferBlock::SourceCycleMissing => 400,
        }
    }

    /// The `{"error": ...}` message for the 400 blocks; `None` for the
    /// 500 (the generic handler renders that body).
    pub fn message(self) -> Option<&'static str> {
        match self {
            TransferBlock::NewCycleMissing => None,
            TransferBlock::NewCycleCompleted => Some(TRANSFER_NEW_CYCLE_COMPLETED_ERROR),
            TransferBlock::SourceCycleMissing => Some(TRANSFER_SOURCE_CYCLE_NOT_FOUND_ERROR),
        }
    }
}

/// Caller-loaded facts for the new-cycle guard: the
/// `TRANSFER_CYCLE_LOOKUP_SQL` outcome for `new_cycle_id`.
pub struct NewCycleFacts {
    /// Whether the lookup returned a row.
    pub exists: bool,
    /// The row's `end_date` as a UTC epoch (`None` = SQL `NULL`, i.e. a
    /// dateless/draft target — the guard passes, `:62`).
    pub end_epoch_secs: Option<i64>,
}

/// The new-cycle guard (transfer file `:59-66`): a missing target is
/// the 500 (ported bug T1); an ended target (`end_date < now`, strict)
/// is the 400. The `now` instant is the caller's `timezone.now()` as a
/// UTC epoch, matching the comparison Python performs.
pub fn check_new_cycle(facts: &NewCycleFacts, now_epoch_secs: i64) -> Result<(), TransferBlock> {
    if !facts.exists {
        return Err(TransferBlock::NewCycleMissing);
    }
    if facts.end_epoch_secs.is_some_and(|end| end < now_epoch_secs) {
        return Err(TransferBlock::NewCycleCompleted);
    }
    Ok(())
}

/// The six old-cycle recounts (transfer file `:411-417`): `COUNT`
/// outputs, never `NULL`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferCounts {
    /// `total_issues`.
    pub total: i64,
    /// `completed_issues`.
    pub completed: i64,
    /// `cancelled_issues`.
    pub cancelled: i64,
    /// `started_issues`.
    pub started: i64,
    /// `unstarted_issues`.
    pub unstarted: i64,
    /// `backlog_issues`.
    pub backlog: i64,
}

/// One assignee estimate-distribution row, serialized per transfer file
/// `:219-229`: `assignee_id` is `None` exactly when the group key is
/// SQL `NULL` (`str(...) if ... else None`, `:222`); the sums are
/// `None` exactly when `SUM` returns `NULL`.
#[derive(Debug, Clone, PartialEq)]
pub struct AssigneeEstimateRow {
    /// `display_name` (SQL `NULL` → `None`).
    pub display_name: Option<String>,
    /// Stringified `assignee_id` (SQL `NULL` → `None`).
    pub assignee_id: Option<String>,
    /// `avatar_url` (SQL `NULL` → `None`).
    pub avatar_url: Option<String>,
    /// `total_estimates`.
    pub total_estimates: Option<f64>,
    /// `completed_estimates`.
    pub completed_estimates: Option<f64>,
    /// `pending_estimates`.
    pub pending_estimates: Option<f64>,
}

/// One label estimate-distribution row (transfer file `:274-284`).
#[derive(Debug, Clone, PartialEq)]
pub struct LabelEstimateRow {
    /// `label_name` (SQL `NULL` → `None`).
    pub label_name: Option<String>,
    /// `color` (SQL `NULL` → `None`).
    pub color: Option<String>,
    /// Stringified `label_id` (SQL `NULL` → `None`).
    pub label_id: Option<String>,
    /// `total_estimates`.
    pub total_estimates: Option<f64>,
    /// `completed_estimates`.
    pub completed_estimates: Option<f64>,
    /// `pending_estimates`.
    pub pending_estimates: Option<f64>,
}

/// One assignee issue-distribution row (transfer file `:338-348`).
/// `COUNT` outputs are never `NULL`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssigneeIssueRow {
    /// `display_name` (SQL `NULL` → `None`).
    pub display_name: Option<String>,
    /// Stringified `assignee_id` (SQL `NULL` → `None`).
    pub assignee_id: Option<String>,
    /// `avatar_url` (SQL `NULL` → `None`).
    pub avatar_url: Option<String>,
    /// `total_issues`.
    pub total_issues: i64,
    /// `completed_issues`.
    pub completed_issues: i64,
    /// `pending_issues`.
    pub pending_issues: i64,
}

/// One label issue-distribution row (transfer file `:387-397`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelIssueRow {
    /// `label_name` (SQL `NULL` → `None`).
    pub label_name: Option<String>,
    /// `color` (SQL `NULL` → `None`).
    pub color: Option<String>,
    /// Stringified `label_id` (SQL `NULL` → `None`).
    pub label_id: Option<String>,
    /// `total_issues`.
    pub total_issues: i64,
    /// `completed_issues`.
    pub completed_issues: i64,
    /// `pending_issues`.
    pub pending_issues: i64,
}

/// The estimate branch of the snapshot (transfer file `:423-431`):
/// present exactly when `estimate_type` is true, else the snapshot
/// carries `"estimate_distribution": {}`.
pub struct EstimateBranch<'a> {
    /// `TRANSFER_LABEL_ESTIMATE_SQL` rows, in row order.
    pub label_estimates: &'a [LabelEstimateRow],
    /// `TRANSFER_ASSIGNEE_ESTIMATE_SQL` rows, in row order.
    pub assignee_estimates: &'a [AssigneeEstimateRow],
    /// `burndown_plot(..., plot_type="points")` output, caller-supplied.
    pub completion_chart: serde_json::Value,
}

/// Inputs to [`build_progress_snapshot`]: every fact the
/// `progress_snapshot` dict reads (transfer file `:411-432`), already
/// loaded by the caller.
pub struct ProgressSnapshotInput<'a> {
    /// The old-cycle recounts (`TRANSFER_OLD_CYCLE_SQL`).
    pub counts: TransferCounts,
    /// `TRANSFER_LABEL_ISSUE_SQL` rows, in row order.
    pub label_issues: &'a [LabelIssueRow],
    /// `TRANSFER_ASSIGNEE_ISSUE_SQL` rows, in row order.
    pub assignee_issues: &'a [AssigneeIssueRow],
    /// `burndown_plot(..., plot_type="issues")` output, caller-supplied.
    pub completion_chart: serde_json::Value,
    /// `None` renders `"estimate_distribution": {}`.
    pub estimates: Option<EstimateBranch<'a>>,
}

fn opt_json(value: &Option<String>) -> serde_json::Value {
    match value {
        Some(text) => serde_json::Value::String(text.clone()),
        None => serde_json::Value::Null,
    }
}

fn opt_f64(value: Option<f64>) -> serde_json::Value {
    match value {
        Some(number) => serde_json::json!(number),
        None => serde_json::Value::Null,
    }
}

/// Assemble the `progress_snapshot` dict (transfer file `:411-432`).
///
/// Key order follows the Python dict; Postgres `jsonb` normalizes key
/// order on write anyway, so only membership and values are the
/// contract. Row order within each distribution is the SQL `ORDER BY`
/// order — load-bearing, preserved by the caller's row slices.
pub fn build_progress_snapshot(input: &ProgressSnapshotInput) -> serde_json::Value {
    let labels: Vec<serde_json::Value> = input
        .label_issues
        .iter()
        .map(|row| {
            serde_json::json!({
                "label_name": opt_json(&row.label_name),
                "color": opt_json(&row.color),
                "label_id": opt_json(&row.label_id),
                "total_issues": row.total_issues,
                "completed_issues": row.completed_issues,
                "pending_issues": row.pending_issues,
            })
        })
        .collect();
    let assignees: Vec<serde_json::Value> = input
        .assignee_issues
        .iter()
        .map(|row| {
            serde_json::json!({
                "display_name": opt_json(&row.display_name),
                "assignee_id": opt_json(&row.assignee_id),
                "avatar_url": opt_json(&row.avatar_url),
                "total_issues": row.total_issues,
                "completed_issues": row.completed_issues,
                "pending_issues": row.pending_issues,
            })
        })
        .collect();
    let estimate_distribution = match &input.estimates {
        None => serde_json::json!({}),
        Some(branch) => {
            let labels: Vec<serde_json::Value> = branch
                .label_estimates
                .iter()
                .map(|row| {
                    serde_json::json!({
                        "label_name": opt_json(&row.label_name),
                        "color": opt_json(&row.color),
                        "label_id": opt_json(&row.label_id),
                        "total_estimates": opt_f64(row.total_estimates),
                        "completed_estimates": opt_f64(row.completed_estimates),
                        "pending_estimates": opt_f64(row.pending_estimates),
                    })
                })
                .collect();
            let assignees: Vec<serde_json::Value> = branch
                .assignee_estimates
                .iter()
                .map(|row| {
                    serde_json::json!({
                        "display_name": opt_json(&row.display_name),
                        "assignee_id": opt_json(&row.assignee_id),
                        "avatar_url": opt_json(&row.avatar_url),
                        "total_estimates": opt_f64(row.total_estimates),
                        "completed_estimates": opt_f64(row.completed_estimates),
                        "pending_estimates": opt_f64(row.pending_estimates),
                    })
                })
                .collect();
            serde_json::json!({
                "labels": labels,
                "assignees": assignees,
                "completion_chart": branch.completion_chart,
            })
        }
    };
    serde_json::json!({
        "total_issues": input.counts.total,
        "completed_issues": input.counts.completed,
        "cancelled_issues": input.counts.cancelled,
        "started_issues": input.counts.started,
        "unstarted_issues": input.counts.unstarted,
        "backlog_issues": input.counts.backlog,
        "distribution": {
            "labels": labels,
            "assignees": assignees,
            "completion_chart": input.completion_chart,
        },
        "estimate_distribution": estimate_distribution,
    })
}

/// One move triple for the `update_cycle_issue_activity` list
/// (transfer file `:450-456`): `{"old_cycle_id", "new_cycle_id",
/// "issue_id"}`, all stringified. The caller collects one per
/// `TRANSFER_MOVE_SELECT_SQL` row, in row order; the `current_instance`
/// JSON assembly and the `issue_activity` call are tasks-owned
/// (PIDASHCONV-310), so this struct carries no serialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferMoveEntry {
    /// Stringified source cycle id.
    pub old_cycle_id: String,
    /// Stringified target cycle id.
    pub new_cycle_id: String,
    /// Stringified moved issue id.
    pub issue_id: String,
}

/// Shape one move triple (transfer file `:450-456`).
pub fn transfer_move_entry(
    cycle_id: &str,
    new_cycle_id: &str,
    issue_id: &str,
) -> TransferMoveEntry {
    TransferMoveEntry {
        old_cycle_id: cycle_id.to_owned(),
        new_cycle_id: new_cycle_id.to_owned(),
        issue_id: issue_id.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_parse_mirrors_django() {
        // Kwarg chains: absent → "-created_at".
        assert_eq!(
            parse_order(None, QUERYSET_ORDER_DEFAULT),
            OrderBy::new("created_at", true)
        );
        // GET chain: absent → "created_at" ascending.
        assert_eq!(
            parse_order(None, ISSUE_GET_ORDER_DEFAULT),
            OrderBy::new("created_at", false)
        );
        // Leading '-' selects descending; anything else is ascending.
        assert_eq!(
            parse_order(Some("-name"), QUERYSET_ORDER_DEFAULT),
            OrderBy::new("name", true)
        );
        assert_eq!(
            parse_order(Some("name"), QUERYSET_ORDER_DEFAULT),
            OrderBy::new("name", false)
        );
        // Passthrough: unknown columns are the database's problem, like
        // Django's FieldError-at-evaluation.
        assert_eq!(
            parse_order(Some("no_such_col"), QUERYSET_ORDER_DEFAULT),
            OrderBy::new("no_such_col", false)
        );
    }

    #[test]
    fn cycle_view_parse_and_pagination() {
        // Absent → "all".
        assert_eq!(parse_cycle_view(None), CycleView::All);
        assert_eq!(parse_cycle_view(Some("all")), CycleView::All);
        assert_eq!(parse_cycle_view(Some("current")), CycleView::Current);
        assert_eq!(parse_cycle_view(Some("upcoming")), CycleView::Upcoming);
        assert_eq!(parse_cycle_view(Some("completed")), CycleView::Completed);
        assert_eq!(parse_cycle_view(Some("draft")), CycleView::Draft);
        assert_eq!(parse_cycle_view(Some("incomplete")), CycleView::Incomplete);
        // Unknown (including wrong case) falls through to the plain list.
        assert_eq!(parse_cycle_view(Some("bogus")), CycleView::All);
        assert_eq!(parse_cycle_view(Some("Current")), CycleView::All);
        assert_eq!(parse_cycle_view(Some("")), CycleView::All);
        // Only `current` skips the paginated envelope (ported bug T2).
        for view in [
            CycleView::All,
            CycleView::Upcoming,
            CycleView::Completed,
            CycleView::Draft,
            CycleView::Incomplete,
        ] {
            assert!(cycle_view_is_paginated(view), "{view:?}");
        }
        assert!(!cycle_view_is_paginated(CycleView::Current));
    }

    #[test]
    fn shape_table() {
        // Archived filter per shape (cycle.py:197,469,630; issue paths: none).
        assert_eq!(
            shape_archived_filter(ReadShape::CycleList),
            ArchivedFilter::Live
        );
        assert_eq!(
            shape_archived_filter(ReadShape::CycleDetail),
            ArchivedFilter::Live
        );
        assert_eq!(
            shape_archived_filter(ReadShape::ArchivedCycleList),
            ArchivedFilter::Archived
        );
        for shape in [
            ReadShape::CycleIssueQueryset,
            ReadShape::CycleIssueListGet,
            ReadShape::CycleIssueDetail,
        ] {
            assert_eq!(shape_archived_filter(shape), ArchivedFilter::Any);
        }
        // Every queryset chain needs the acting user (the asymmetry vs
        // the module reads); only the GET shape and the lookup skip it.
        for shape in [
            ReadShape::CycleList,
            ReadShape::CycleDetail,
            ReadShape::ArchivedCycleList,
            ReadShape::CycleIssueQueryset,
        ] {
            assert!(shape_needs_member(shape), "{shape:?}");
        }
        assert!(!shape_needs_member(ReadShape::CycleIssueListGet));
        assert!(!shape_needs_member(ReadShape::CycleIssueDetail));
        // DISTINCT on every queryset chain; GET shape and lookup skip it.
        for shape in [
            ReadShape::CycleList,
            ReadShape::CycleDetail,
            ReadShape::ArchivedCycleList,
            ReadShape::CycleIssueQueryset,
        ] {
            assert!(shape_is_distinct(shape), "{shape:?}");
        }
        assert!(!shape_is_distinct(ReadShape::CycleIssueListGet));
        assert!(!shape_is_distinct(ReadShape::CycleIssueDetail));
        // Order defaults: descending everywhere except the GET shape.
        for shape in [
            ReadShape::CycleList,
            ReadShape::CycleDetail,
            ReadShape::ArchivedCycleList,
            ReadShape::CycleIssueQueryset,
            ReadShape::CycleIssueDetail,
        ] {
            assert_eq!(shape_order_default(shape), "-created_at", "{shape:?}");
        }
        assert_eq!(
            shape_order_default(ReadShape::CycleIssueListGet),
            "created_at"
        );
    }

    #[test]
    fn issue_detail_get_is_routed() {
        // api/urls/cycle.py:38-42 wires get + delete (the reverse of the
        // module asymmetry).
        assert_eq!(CYCLE_ISSUE_DETAIL_ROUTED_METHODS, &["get", "delete"]);
    }

    #[test]
    fn new_cycle_guard_table() {
        // Missing target → 500 (ported bug T1).
        let missing = NewCycleFacts {
            exists: false,
            end_epoch_secs: None,
        };
        assert_eq!(
            check_new_cycle(&missing, 1_700_000_000),
            Err(TransferBlock::NewCycleMissing)
        );
        // Ended target (strict `<`) → 400.
        let ended = NewCycleFacts {
            exists: true,
            end_epoch_secs: Some(1_699_999_999),
        };
        assert_eq!(
            check_new_cycle(&ended, 1_700_000_000),
            Err(TransferBlock::NewCycleCompleted)
        );
        // Boundary: end == now proceeds (strict comparison).
        let boundary = NewCycleFacts {
            exists: true,
            end_epoch_secs: Some(1_700_000_000),
        };
        assert!(check_new_cycle(&boundary, 1_700_000_000).is_ok());
        // Future end and dateless targets proceed.
        for facts in [
            NewCycleFacts {
                exists: true,
                end_epoch_secs: Some(1_700_000_001),
            },
            NewCycleFacts {
                exists: true,
                end_epoch_secs: None,
            },
        ] {
            assert!(check_new_cycle(&facts, 1_700_000_000).is_ok());
        }
    }

    #[test]
    fn transfer_block_status_and_messages() {
        assert_eq!(TransferBlock::NewCycleMissing.status_code(), 500);
        assert_eq!(TransferBlock::NewCycleMissing.message(), None);
        assert_eq!(TransferBlock::NewCycleCompleted.status_code(), 400);
        assert_eq!(
            TransferBlock::NewCycleCompleted.message(),
            Some("The cycle where the issues are transferred is already completed")
        );
        assert_eq!(TransferBlock::SourceCycleMissing.status_code(), 400);
        assert_eq!(
            TransferBlock::SourceCycleMissing.message(),
            Some("Source cycle not found")
        );
    }

    #[test]
    fn snapshot_golden_with_estimates() {
        // Transfer file `:411-432` with the estimate branch on.
        let input = ProgressSnapshotInput {
            counts: TransferCounts {
                total: 4,
                completed: 1,
                cancelled: 0,
                started: 2,
                unstarted: 1,
                backlog: 0,
            },
            label_issues: &[LabelIssueRow {
                label_name: Some("bug".to_owned()),
                color: Some("#ff0000".to_owned()),
                label_id: Some("61111111-1111-1111-4111-111111111101".to_owned()),
                total_issues: 2,
                completed_issues: 1,
                pending_issues: 1,
            }],
            assignee_issues: &[AssigneeIssueRow {
                display_name: Some("Ada".to_owned()),
                assignee_id: Some("71111111-1111-1111-4111-111111111101".to_owned()),
                avatar_url: None,
                total_issues: 3,
                completed_issues: 1,
                pending_issues: 2,
            }],
            completion_chart: serde_json::json!({"2026-09-01": 4}),
            estimates: Some(EstimateBranch {
                label_estimates: &[LabelEstimateRow {
                    label_name: Some("bug".to_owned()),
                    color: Some("#ff0000".to_owned()),
                    label_id: Some("61111111-1111-1111-4111-111111111101".to_owned()),
                    total_estimates: Some(5.0),
                    completed_estimates: Some(3.0),
                    pending_estimates: Some(2.0),
                }],
                assignee_estimates: &[AssigneeEstimateRow {
                    display_name: None,
                    assignee_id: None,
                    avatar_url: None,
                    total_estimates: None,
                    completed_estimates: None,
                    pending_estimates: None,
                }],
                completion_chart: serde_json::json!({"2026-09-01": 5.0}),
            }),
        };
        assert_eq!(
            build_progress_snapshot(&input),
            serde_json::json!({
                "total_issues": 4,
                "completed_issues": 1,
                "cancelled_issues": 0,
                "started_issues": 2,
                "unstarted_issues": 1,
                "backlog_issues": 0,
                "distribution": {
                    "labels": [{
                        "label_name": "bug",
                        "color": "#ff0000",
                        "label_id": "61111111-1111-1111-4111-111111111101",
                        "total_issues": 2,
                        "completed_issues": 1,
                        "pending_issues": 1,
                    }],
                    "assignees": [{
                        "display_name": "Ada",
                        "assignee_id": "71111111-1111-1111-4111-111111111101",
                        "avatar_url": null,
                        "total_issues": 3,
                        "completed_issues": 1,
                        "pending_issues": 2,
                    }],
                    "completion_chart": {"2026-09-01": 4},
                },
                "estimate_distribution": {
                    "labels": [{
                        "label_name": "bug",
                        "color": "#ff0000",
                        "label_id": "61111111-1111-1111-4111-111111111101",
                        "total_estimates": 5.0,
                        "completed_estimates": 3.0,
                        "pending_estimates": 2.0,
                    }],
                    "assignees": [{
                        "display_name": null,
                        "assignee_id": null,
                        "avatar_url": null,
                        "total_estimates": null,
                        "completed_estimates": null,
                        "pending_estimates": null,
                    }],
                    "completion_chart": {"2026-09-01": 5.0},
                },
            })
        );
    }

    #[test]
    fn snapshot_without_estimates_carries_empty_object() {
        // Transfer file `:423-425`: no estimate branch → `{}`.
        let input = ProgressSnapshotInput {
            counts: TransferCounts {
                total: 0,
                completed: 0,
                cancelled: 0,
                started: 0,
                unstarted: 0,
                backlog: 0,
            },
            label_issues: &[],
            assignee_issues: &[],
            completion_chart: serde_json::json!({}),
            estimates: None,
        };
        let snapshot = build_progress_snapshot(&input);
        assert_eq!(
            snapshot.get("estimate_distribution"),
            Some(&serde_json::json!({}))
        );
        assert_eq!(
            snapshot.get("distribution"),
            Some(&serde_json::json!({
                "labels": [],
                "assignees": [],
                "completion_chart": {},
            }))
        );
    }

    #[test]
    fn move_entry_shapes_triple() {
        // Transfer file `:450-456`.
        assert_eq!(
            transfer_move_entry("c1", "c2", "i9"),
            TransferMoveEntry {
                old_cycle_id: "c1".to_owned(),
                new_cycle_id: "c2".to_owned(),
                issue_id: "i9".to_owned(),
            }
        );
    }
}
