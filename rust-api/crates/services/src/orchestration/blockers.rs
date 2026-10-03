#![forbid(unsafe_code)]

//! Blocker lookups (D-12 L3, stage 5).
//!
//! Ports `orchestration/blockers.py` whole — the single place "what is
//! this work item blocked by, and who is waiting on it?" is answered.
//! Informational only: nothing here gates dispatch.
//!
//! Two forms, mirroring `pi_dash.loop.eligibility`:
//!
//! * row form ([`blockers_sql`], [`open_blockers_sql`],
//!   [`has_open_blockers_sql`], [`dependents_sql`],
//!   [`relations_summary`]) for one issue;
//! * bulk form ([`open_blockers_q_sql`]) — an `Exists` predicate for
//!   scans without an N+1.
//!
//! Both share [`blocked_by_edges_sql`] / [`blocking_edges_sql`], so
//! they cannot disagree about what counts as a blocker. The services
//! crate carries no `sqlx` dependency, so — per the D-27/D-30/D-36
//! `queries.rs` precedent — SQL here is text plus symbolic `:name`
//! placeholders; handlers translate each `:name` to a positional `$n`
//! in the statement's `*_PARAMS` order (first appearance) when binding
//! via `sqlx`. Pure shaping over fetched rows ([`summary_item`],
//! [`summary_list`], [`relations_summary`], [`order_rows`],
//! [`open_only`]) mirrors the queryset-side logic so it stays
//! unit-testable with no database.
//!
//! Edge rules (`blockers.py:23-35`): a row `(A, B, "blocked_by")`
//! means "A is blocked by B"; a row stored under the reverse name
//! `(B, A, "blocking")` reads the same way. Soft-deleted relation
//! rows are ignored, self-edges are excluded, the other end must be a
//! live work item (`Issue.issue_objects`: not deleted, archived,
//! draft or triage) in the relation's workspace, and a blocker is
//! *open* until its state group is `completed` or `cancelled`
//! (`review` / `test` still count; no state counts as open).
//!
//! Fixture: FX-ORCH-03 (`rust-api/fixtures/orchestration/fx03_blockers/`:
//! `edges.sql`, `edges.rows.json`, `blockers.golden.json`,
//! `open_blockers_q.sql`, `relations_summary.golden.json`). The
//! recorded SQL is Django-literal (quoted identifiers, `V0`/`U0`
//! aliases, `::uuid` casts), so the replay below pins *semantic*
//! equality — every predicate of the fixture must appear in the
//! generated statement — rather than byte equality.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
//!
//! Existing quirks ported as-is (translation, don't redesign):
//!
//! 1. `_live_relations` (`:60-61`) renders `deleted_at IS NULL` twice
//!    (default manager plus the explicit filter). The port emits it
//!    once: the duplicate is unobservable and the fixtures pin Django's
//!    literal text, which no hand-written statement can match byte for
//!    byte.
//! 2. Stateless issues are live *and* open: the triage exclusion keeps
//!    its `AND <alias>."group" IS NOT NULL` conjunct (dropping it
//!    would wrongly exclude stateless issues), and the open exclusion
//!    likewise keeps NULL groups on the open side.
//! 3. No `DISTINCT` anywhere: the ORM filters outer issue rows with
//!    `id IN (...) OR id IN (...)`, so each issue appears once by
//!    construction even when both a forward and a stored-reversed row
//!    point at it. (The earlier inline port in
//!    `api/src/v1_cycles_modules/cycle.rs` joins edge rows instead and
//!    needs its `DISTINCT`; same result set.)

use serde::{Deserialize, Serialize};

use pidash_db::app_project::models::state::StateGroup;

// ---------------------------------------------------------------------------
// Constants (`blockers.py:47-57`)
// ---------------------------------------------------------------------------

/// `BLOCKED_BY` (`blockers.py:47`,
/// `IssueRelationChoices.BLOCKED_BY.value`).
pub const BLOCKED_BY: &str = "blocked_by";

/// `BLOCKING` (`blockers.py:48`,
/// `IssueRelationChoices._REVERSE_MAPPING[BLOCKED_BY]`).
pub const BLOCKING: &str = "blocking";

/// `CLOSED_STATE_GROUPS` (`blockers.py:50-51`): state groups that mean
/// a blocker no longer holds its dependents back. Order follows the
/// Python source (`COMPLETED`, `CANCELLED`); membership is all that
/// matters. Reuses the existing [`StateGroup`] type — never re-ported.
pub const CLOSED_STATE_GROUPS: &[StateGroup] = &[StateGroup::Completed, StateGroup::Cancelled];

/// `SUMMARY_LIMIT` (`blockers.py:57`): per-direction cap on
/// [`relations_summary`] lists.
pub const SUMMARY_LIMIT: usize = 100;

// ---------------------------------------------------------------------------
// Open rule (`_open`, `blockers.py:109-110`)
// ---------------------------------------------------------------------------

/// `True` when the stored state-group string is in
/// [`CLOSED_STATE_GROUPS`]. Comparison goes through
/// [`StateGroup::as_str`], the single source of the stored strings.
/// An unknown group string is *not* closed — exactly the Django
/// `exclude(state__group__in=...)` semantics.
pub fn is_closed_group(group: &str) -> bool {
    CLOSED_STATE_GROUPS
        .iter()
        .any(|closed| closed.as_str() == group)
}

/// Row mirror of `_open` (`:109-110`): `True` unless the row's state
/// group is closed. `None` (no state) is open — the SQL form keeps
/// its `AND <alias>."group" IS NOT NULL` conjunct for the same reason.
pub fn is_open_group(group: Option<&str>) -> bool {
    group.is_none_or(|g| !is_closed_group(g))
}

// ---------------------------------------------------------------------------
// Edge verdict kernel (pure mirror of the edge rules, `:23-35`)
// ---------------------------------------------------------------------------

/// Facts one relation row plus its target contribute to the
/// included/excluded verdict. Relation *type* and *orientation* are
/// structural (each edge subquery fixes them); the kernel covers the
/// four filters: liveness of the row, the self-edge exclusion,
/// liveness of the target, and the same-workspace rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeFacts {
    /// `deleted_at IS NULL` on the relation row.
    pub relation_live: bool,
    /// `issue_id == related_issue_id`.
    pub self_edge: bool,
    /// Target is a live work item (`Issue.issue_objects` scope).
    pub target_live: bool,
    /// Target's workspace is the relation's workspace.
    pub target_same_workspace: bool,
}

/// `True` when an edge with these facts counts as a blocker edge.
/// Replays `edges.rows.json` (`included` per row).
pub fn edge_included(facts: &EdgeFacts) -> bool {
    facts.relation_live && !facts.self_edge && facts.target_live && facts.target_same_workspace
}

// ---------------------------------------------------------------------------
// SQL fragments
// ---------------------------------------------------------------------------

/// Placeholder translation rule for handlers: replace each `:name`
/// with a positional `$n` in the statement's `*_PARAMS` order (first
/// appearance). `:issue_id` appears twice (forward + stored-reversed
/// subqueries) but binds once.
pub const PLACEHOLDER_RULE: &str = ":name in PARAMS order -> $n";

/// Bind order for every row-form statement in this module
/// ([`blockers_sql`], [`open_blockers_sql`],
/// [`has_open_blockers_sql`], [`dependents_sql`], [`summary_sql`]).
pub const ISSUE_PARAMS: &[&str] = &["issue_id"];

/// Bind order for [`open_blockers_q_sql`]: the bulk predicate takes
/// no parameters — the anchor is the caller's outer column.
pub const OPEN_BLOCKERS_Q_PARAMS: &[&str] = &[];

/// `Issue.issue_objects` liveness predicate
/// (`db/models/issue.py:95-104`), the same fragment shape L2 pins in
/// `db/src/orchestration/querysets.rs`. `i`/`s`/`p` are the
/// issues/states/projects aliases of the enclosing query. The triage
/// exclusion keeps `AND <s>."group" IS NOT NULL` so stateless issues
/// stay live (quirk 2).
pub fn live_issue_predicate(i: &str, s: &str, p: &str) -> String {
    format!(
        "{i}.\"deleted_at\" IS NULL \
         AND NOT ({s}.\"group\" = 'triage' AND {s}.\"group\" IS NOT NULL) \
         AND NOT ({i}.\"archived_at\" IS NOT NULL) \
         AND NOT ({p}.\"archived_at\" IS NOT NULL) \
         AND NOT ({i}.\"is_draft\")"
    )
}

/// SQL mirror of `_open` (`blockers.py:109-110`): excludes the closed
/// groups while keeping NULL-group (stateless) rows on the open side.
/// `s` is the states alias. `IN` order (`cancelled`, `completed`) is
/// the fixture's.
pub fn open_predicate(s: &str) -> String {
    format!("NOT ({s}.\"group\" IN ('cancelled', 'completed') AND {s}.\"group\" IS NOT NULL)")
}

/// `_live_relations` (`blockers.py:60-61`): live relation rows,
/// self-edges excluded. `r` is the `issue_relations` alias. Emits the
/// NULL guard once (quirk 1).
pub fn live_relation_predicate(r: &str) -> String {
    format!("{r}.\"deleted_at\" IS NULL AND NOT ({r}.\"issue_id\" = {r}.\"related_issue_id\")")
}

/// `Issue.issue_objects.all()` as an id subquery: the `targets` half
/// of the edge kernels. Aliases `lt`/`ls`/`lp` never collide with the
/// outer query's `i`/`s`/`p` or the edge aliases `r`/`t`.
pub fn live_targets_sql() -> String {
    format!(
        "SELECT lt.\"id\" FROM \"issues\" lt \
         LEFT OUTER JOIN \"states\" ls ON (lt.\"state_id\" = ls.\"id\") \
         INNER JOIN \"projects\" lp ON (lt.\"project_id\" = lp.\"id\") \
         WHERE ({})",
        live_issue_predicate("lt", "ls", "lp")
    )
}

/// `_open(Issue.issue_objects.all())`: the `targets` half the bulk
/// predicate scans. Same subquery plus [`open_predicate`].
pub fn open_targets_sql() -> String {
    format!(
        "SELECT lt.\"id\" FROM \"issues\" lt \
         LEFT OUTER JOIN \"states\" ls ON (lt.\"state_id\" = ls.\"id\") \
         INNER JOIN \"projects\" lp ON (lt.\"project_id\" = lp.\"id\") \
         WHERE ({} AND {})",
        live_issue_predicate("lt", "ls", "lp"),
        open_predicate("ls")
    )
}

// ---------------------------------------------------------------------------
// Edge kernels (`_blocked_by_edges` / `_blocking_edges`, `:64-106`)
// ---------------------------------------------------------------------------

/// One edge subquery: relation rows whose anchor end equals
/// `anchor_expr` under `relation_type`, with the target end drawn
/// from `targets_sql` and joined (`t`) for the workspace match.
/// Predicate order follows the ORM: liveness, anchor, targets, workspace,
/// type. `anchor_column` is the anchored end, `target_column` the end
/// the target sits on (also the projected column).
fn edge_subquery_sql(
    anchor_column: &str,
    anchor_expr: &str,
    relation_type: &str,
    target_column: &str,
    targets_sql: &str,
) -> String {
    format!(
        "SELECT r.\"{target_column}\" FROM \"issue_relations\" r \
         INNER JOIN \"issues\" t ON (r.\"{target_column}\" = t.\"id\") \
         WHERE ({} AND r.\"{anchor_column}\" = {anchor_expr} \
         AND r.\"{target_column}\" IN ({targets_sql}) \
         AND t.\"workspace_id\" = r.\"workspace_id\" \
         AND r.\"relation_type\" = '{relation_type}')",
        live_relation_predicate("r"),
    )
}

/// `_blocked_by_edges` (`blockers.py:64-84`): relation rows saying
/// `dependent` is blocked by an issue in `targets_sql`. `dependent`
/// is the `:issue_id` placeholder (row form) or the caller's outer
/// column (bulk form) — the port of "an issue id or an `OuterRef`".
/// Returns `(forward, stored_reversed)`: in `forward` the blocker is
/// `related_issue`, in `stored_reversed` it is `issue`.
pub fn blocked_by_edges_sql(dependent: &str, targets_sql: &str) -> (String, String) {
    let forward = edge_subquery_sql(
        "issue_id",
        dependent,
        BLOCKED_BY,
        "related_issue_id",
        targets_sql,
    );
    let stored_reversed = edge_subquery_sql(
        "related_issue_id",
        dependent,
        BLOCKING,
        "issue_id",
        targets_sql,
    );
    (forward, stored_reversed)
}

/// `_blocking_edges` (`blockers.py:87-106`): relation rows saying
/// `blocker` blocks an issue in `targets_sql`. Mirror of
/// [`blocked_by_edges_sql`]: in `forward` the dependent is `issue`,
/// in `stored_reversed` it is `related_issue`.
pub fn blocking_edges_sql(blocker: &str, targets_sql: &str) -> (String, String) {
    let forward = edge_subquery_sql(
        "related_issue_id",
        blocker,
        BLOCKED_BY,
        "issue_id",
        targets_sql,
    );
    let stored_reversed = edge_subquery_sql(
        "issue_id",
        blocker,
        BLOCKING,
        "related_issue_id",
        targets_sql,
    );
    (forward, stored_reversed)
}

// ---------------------------------------------------------------------------
// Row-form statements (`blockers_queryset` / `dependents_queryset`,
// `_ordered`, `blockers`, `open_blockers`, `has_open_blockers`,
// `dependents`, `:113-149`)
// ---------------------------------------------------------------------------

/// Projected columns of the row-form statements: the issue id plus
/// exactly the ordering and summary fields (`_ordered` + `_summary_item`).
/// The ORM hydrates full issue/project/state rows via `select_related`;
/// handlers need only these columns, and the unselected ones are
/// unobservable to this module's consumers.
pub const BLOCKER_ROW_COLUMNS: &str =
    "i.\"id\", i.\"sequence_id\", p.\"identifier\", s.\"name\", s.\"group\"";

/// Shared FROM/WHERE skeleton: live outer issues narrowed to the
/// union of the two edge subqueries (`Q(id__in=...) | Q(id__in=...)`).
/// `open_only` appends [`open_predicate`] (the `_open` wrapper).
fn row_statement_skeleton(edge_sql: &(String, String), open_only: bool) -> String {
    let open = if open_only {
        format!(" AND {}", open_predicate("s"))
    } else {
        String::new()
    };
    format!(
        "FROM \"issues\" i LEFT OUTER JOIN \"states\" s ON (i.\"state_id\" = s.\"id\") \
         INNER JOIN \"projects\" p ON (i.\"project_id\" = p.\"id\") \
         WHERE ({} AND (i.\"id\" IN ({}) OR i.\"id\" IN ({})){})",
        live_issue_predicate("i", "s", "p"),
        edge_sql.0,
        edge_sql.1,
        open,
    )
}

/// `_ordered` (`:129-130`): project identifier, then sequence.
pub const ORDER_BY: &str = "ORDER BY p.\"identifier\" ASC, i.\"sequence_id\" ASC";

/// `blockers_queryset` + `_ordered` (`:113-118`, `:129-130`): every
/// live `blocked_by` target of `:issue_id`, open or resolved.
/// Binds [`ISSUE_PARAMS`].
pub fn blockers_sql() -> String {
    format!(
        "SELECT {} {} {}",
        BLOCKER_ROW_COLUMNS,
        row_statement_skeleton(
            &blocked_by_edges_sql(":issue_id", &live_targets_sql()),
            false
        ),
        ORDER_BY,
    )
}

/// `open_blockers` (`:138-140`): [`blockers_sql`] plus [`open_predicate`].
/// Binds [`ISSUE_PARAMS`].
pub fn open_blockers_sql() -> String {
    format!(
        "SELECT {} {} {}",
        BLOCKER_ROW_COLUMNS,
        row_statement_skeleton(
            &blocked_by_edges_sql(":issue_id", &live_targets_sql()),
            true
        ),
        ORDER_BY,
    )
}

/// `has_open_blockers` (`:143-144`): `SELECT 1 ... LIMIT 1` over the
/// open set — always the full set, never a capped list.
/// Binds [`ISSUE_PARAMS`].
pub fn has_open_blockers_sql() -> String {
    format!(
        "SELECT 1 AS \"a\" {} LIMIT 1",
        row_statement_skeleton(
            &blocked_by_edges_sql(":issue_id", &live_targets_sql()),
            true
        ),
    )
}

/// `dependents_queryset` + `_ordered` (`:121-126`, `:129-130`): every
/// live issue listing `:issue_id` under `blocked_by`, any state.
/// Binds [`ISSUE_PARAMS`].
pub fn dependents_sql() -> String {
    format!(
        "SELECT {} {} {}",
        BLOCKER_ROW_COLUMNS,
        row_statement_skeleton(&blocking_edges_sql(":issue_id", &live_targets_sql()), false),
        ORDER_BY,
    )
}

// ---------------------------------------------------------------------------
// Bulk predicate (`open_blockers_q`, `:152-160`)
// ---------------------------------------------------------------------------

/// Default outer anchor of [`open_blockers_q_sql`]: `OuterRef("pk")`
/// over an `Issue` queryset resolves to the outer `issues.id` (as the
/// `open_blockers_q.sql` fixture's bulk scan shows).
pub const OPEN_BLOCKERS_Q_DEFAULT_OUTER: &str = "\"issues\".\"id\"";

/// `open_blockers_q` (`:152-160`): `Q(Exists(forward)) |
/// Q(Exists(stored_reversed))` over [`open_targets_sql`], anchored on
/// `outer_column`. `outer_column` is the caller's outer reference —
/// [`OPEN_BLOCKERS_Q_DEFAULT_OUTER`] for `Issue` scans (the
/// `issue_ref="pk"` default), e.g. `"issue_agent_ticker"."issue_id"`
/// for `open_blockers_q("issue_id")`. Same edge kernels as the row
/// form, so the two cannot disagree. Binds nothing
/// ([`OPEN_BLOCKERS_Q_PARAMS`]).
pub fn open_blockers_q_sql_for(outer_column: &str) -> String {
    let (forward, stored_reversed) = blocked_by_edges_sql(outer_column, &open_targets_sql());
    format!("(EXISTS({forward} LIMIT 1) OR EXISTS({stored_reversed} LIMIT 1))")
}

/// [`open_blockers_q_sql_for`] with the `issue_ref="pk"` default.
pub fn open_blockers_q_sql() -> String {
    open_blockers_q_sql_for(OPEN_BLOCKERS_Q_DEFAULT_OUTER)
}

// ---------------------------------------------------------------------------
// Summary statements (`_summary_list`, `:172-181`)
// ---------------------------------------------------------------------------

/// The `_resolved` annotation (`:173-178`): `CASE WHEN <closed> THEN 1
/// ELSE 0 END`. A NULL group takes the `ELSE` branch, so stateless
/// rows sort open-first. `s` is the states alias.
pub fn resolved_case_sql(s: &str) -> String {
    format!(
        "CASE WHEN {s}.\"group\" IN ('cancelled', 'completed') THEN 1 ELSE 0 END AS \"_resolved\""
    )
}

/// Open-first order (`:180`): resolved flag, then project identifier,
/// then sequence.
pub const SUMMARY_ORDER_BY: &str = "ORDER BY \"_resolved\", p.\"identifier\", i.\"sequence_id\"";

/// `_summary_list` (`:172-181`) over [`blockers_sql`]
/// (`blocking = false`) or [`dependents_sql`] (`blocking = true`):
/// same row set, open-first, capped at [`SUMMARY_LIMIT`].
/// `relations_summary` runs this twice plus [`has_open_blockers_sql`]
/// — the fixture's 3 statements. Binds [`ISSUE_PARAMS`].
pub fn summary_sql(blocking: bool) -> String {
    let edges = if blocking {
        blocking_edges_sql(":issue_id", &live_targets_sql())
    } else {
        blocked_by_edges_sql(":issue_id", &live_targets_sql())
    };
    format!(
        "SELECT {}, {} {} {} LIMIT {SUMMARY_LIMIT}",
        BLOCKER_ROW_COLUMNS,
        resolved_case_sql("s"),
        row_statement_skeleton(&edges, false),
        SUMMARY_ORDER_BY,
    )
}

// ---------------------------------------------------------------------------
// Rows + shaping (`_ordered`, `_open`, `_summary_item`, `_summary_list`,
// `relations_summary`, `:129-130`, `:109-110`, `:163-198`)
// ---------------------------------------------------------------------------

/// One row of the row-form statements, in [`BLOCKER_ROW_COLUMNS`]
/// order: the issue id plus the ordering and summary fields.
/// `state_name` / `state_group` are `None` together when the issue
/// has no state (the `LEFT OUTER JOIN`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockerRow {
    /// `issues.id`.
    pub issue_id: uuid::Uuid,
    /// `issues.sequence_id` (`IntegerField`).
    pub sequence_id: i32,
    /// `projects.identifier`.
    pub project_identifier: String,
    /// `states.name`, or `None` without a state.
    pub state_name: Option<String>,
    /// `states.group`, or `None` without a state.
    pub state_group: Option<String>,
}

/// Row mirror of `_open` (`:109-110`).
pub fn is_open_row(row: &BlockerRow) -> bool {
    is_open_group(row.state_group.as_deref())
}

/// Row mirror of `_ordered` (`:129-130`): sorts in place by project
/// identifier, then sequence. Stable, so rows already in SQL order
/// keep it.
pub fn order_rows(rows: &mut [BlockerRow]) {
    rows.sort_by(|a, b| {
        (&a.project_identifier, a.sequence_id).cmp(&(&b.project_identifier, b.sequence_id))
    });
}

/// Row mirror of `_open` over a fetched set (`open_blockers`,
/// `:138-140`): keeps the open rows, preserving order.
pub fn open_only(rows: &[BlockerRow]) -> Vec<BlockerRow> {
    rows.iter().filter(|r| is_open_row(r)).cloned().collect()
}

/// One `_summary_item` (`:163-169`): `{identifier, state,
/// state_group}` — no titles or bodies. Field order is the Python
/// dict order, so `serde_json` renders byte-identical JSON. All three
/// keys are always present: a stateless row renders explicit `null`s
/// (the `None`-vs-absent-key trap).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryItem {
    /// `f"{project.identifier}-{sequence_id}"`.
    pub identifier: String,
    /// State name, or `None` without a state.
    pub state: Option<String>,
    /// State group, or `None` without a state.
    pub state_group: Option<String>,
}

/// `_summary_item` (`:163-169`).
pub fn summary_item(row: &BlockerRow) -> SummaryItem {
    SummaryItem {
        identifier: format!("{}-{}", row.project_identifier, row.sequence_id),
        state: row.state_name.clone(),
        state_group: row.state_group.clone(),
    }
}

/// `_summary_list` (`:172-181`): open rows first, then project
/// identifier, then sequence, capped at [`SUMMARY_LIMIT`]. Idempotent
/// over rows already shaped by [`summary_sql`] (stable sort + cap).
pub fn summary_list(rows: &[BlockerRow]) -> Vec<SummaryItem> {
    let mut ordered: Vec<&BlockerRow> = rows.iter().collect();
    ordered.sort_by(|a, b| {
        (!is_open_row(a), &a.project_identifier, a.sequence_id).cmp(&(
            !is_open_row(b),
            &b.project_identifier,
            b.sequence_id,
        ))
    });
    ordered
        .into_iter()
        .take(SUMMARY_LIMIT)
        .map(summary_item)
        .collect()
}

/// The `relations_summary` value (`:186-198`): per-direction lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationDirections {
    /// `_summary_list(blockers_queryset(issue))`.
    pub blocked_by: Vec<SummaryItem>,
    /// `_summary_list(dependents_queryset(issue))`.
    pub blocking: Vec<SummaryItem>,
}

/// `relations_summary` (`:184-198`): `{"relations_summary":
/// {"blocked_by": [...], "blocking": [...]}, "has_open_blockers":
/// bool}`. Key order is the Python dict order. `has_open_blockers`
/// is computed over the full set (the caller runs
/// [`has_open_blockers_sql`]), never over the capped lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationsSummary {
    /// The two capped, open-first lists.
    pub relations_summary: RelationDirections,
    /// Open blockers over the full set.
    pub has_open_blockers: bool,
}

/// `relations_summary` (`:184-198`) over fetched rows.
pub fn relations_summary(
    blocked_by: &[BlockerRow],
    blocking: &[BlockerRow],
    has_open_blockers: bool,
) -> RelationsSummary {
    RelationsSummary {
        relations_summary: RelationDirections {
            blocked_by: summary_list(blocked_by),
            blocking: summary_list(blocking),
        },
        has_open_blockers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Load one FX-ORCH-03 fixture file.
    fn fixture(name: &str) -> serde_json::Value {
        let path = format!(
            "{}/../../fixtures/orchestration/fx03_blockers/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&path).expect("fx03 fixture exists");
        serde_json::from_str(&text).expect("fx03 fixture parses")
    }

    /// Lowercase, unquote, collapse whitespace: the recorded SQL is
    /// Django-literal (quoted identifiers, `V0`/`U0` aliases), so
    /// replay compares normalized predicates, not bytes.
    fn normalize_sql(sql: &str) -> String {
        sql.replace('"', "")
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Assert the same semantic predicate appears in the fixture's
    /// dialect (`fixture_needle`, `V0`/`U0` aliases, full table names)
    /// and in ours (`ours_needle`, `r`/`t`/`lt` aliases, `i`/`s`/`p`
    /// outer aliases).
    fn assert_same_predicate(
        fixture_sql: &str,
        ours: &str,
        fixture_needle: &str,
        ours_needle: &str,
        what: &str,
    ) {
        assert!(
            normalize_sql(fixture_sql).contains(&normalize_sql(fixture_needle)),
            "fixture lost predicate for {what}: {fixture_needle}"
        );
        assert!(
            normalize_sql(ours).contains(&normalize_sql(ours_needle)),
            "port lost predicate for {what}: {ours_needle}"
        );
    }

    /// Build a [`BlockerRow`] from a golden `{identifier, state,
    /// state_group}` item. `n` feeds a deterministic issue id.
    fn row_from_golden(item: &serde_json::Value, n: u128) -> BlockerRow {
        let identifier = item["identifier"].as_str().expect("identifier is str");
        let (project, seq) = identifier.rsplit_once('-').expect("identifier is PROJ-seq");
        BlockerRow {
            issue_id: uuid::Uuid::from_u128(n),
            sequence_id: seq.parse().expect("sequence_id is int"),
            project_identifier: project.to_owned(),
            state_name: item["state"].as_str().map(str::to_owned),
            state_group: item["state_group"].as_str().map(str::to_owned),
        }
    }

    /// Golden items -> rows, preserving golden order.
    fn rows_from_golden(items: &serde_json::Value) -> Vec<BlockerRow> {
        items
            .as_array()
            .expect("golden list is array")
            .iter()
            .enumerate()
            .map(|(n, item)| row_from_golden(item, n as u128 + 1))
            .collect()
    }

    fn identifiers(items: &[SummaryItem]) -> Vec<&str> {
        items.iter().map(|i| i.identifier.as_str()).collect()
    }

    fn row_identifiers(rows: &[BlockerRow]) -> Vec<String> {
        rows.iter().map(|r| summary_item(r).identifier).collect()
    }

    /// `blockers.golden.json` constants pin the module constants
    /// (`:47-51`, `:57`).
    #[test]
    fn constants_match_fixture() {
        let golden = fixture("blockers.golden.json");
        let constants = &golden["constants"];
        assert_eq!(constants["BLOCKED_BY"].as_str(), Some(BLOCKED_BY));
        assert_eq!(constants["BLOCKING"].as_str(), Some(BLOCKING));
        assert_eq!(
            constants["SUMMARY_LIMIT"].as_u64(),
            Some(SUMMARY_LIMIT as u64)
        );
        let mut closed: Vec<&str> = CLOSED_STATE_GROUPS.iter().map(|g| g.as_str()).collect();
        closed.sort_unstable();
        let mut expected: Vec<&str> = constants["CLOSED_STATE_GROUPS"]
            .as_array()
            .expect("closed groups is array")
            .iter()
            .map(|v| v.as_str().expect("group is str"))
            .collect();
        expected.sort_unstable();
        assert_eq!(closed, expected);
    }

    /// The `open_rule` matrix (`blockers.golden.json`) drives
    /// [`is_open_group`]: review/test/no-state open,
    /// completed/cancelled closed — plus unknown groups stay open
    /// (Django `exclude` semantics).
    #[test]
    fn open_rule_matrix() {
        let golden = fixture("blockers.golden.json");
        let rule = &golden["open_rule"];
        assert_eq!(
            is_open_group(Some("review")),
            rule["review_is_open"].as_bool().unwrap()
        );
        assert_eq!(
            is_open_group(Some("test")),
            rule["test_is_open"].as_bool().unwrap()
        );
        assert_eq!(
            is_open_group(None),
            rule["no_state_is_open"].as_bool().unwrap()
        );
        assert_eq!(
            !is_open_group(Some("completed")),
            rule["completed_is_closed"].as_bool().unwrap()
        );
        assert_eq!(
            !is_open_group(Some("cancelled")),
            rule["cancelled_is_closed"].as_bool().unwrap()
        );
        // Every other known group is open; unknown strings too.
        for group in ["backlog", "unstarted", "started", "triage", "wat"] {
            assert!(is_open_group(Some(group)), "{group} is open");
        }
        assert!(is_closed_group("completed"));
        assert!(is_closed_group("cancelled"));
        assert!(!is_closed_group("review"));
    }

    /// All 14 `edges.rows.json` rows replay through [`edge_included`]:
    /// orientation comes from the fixture's own
    /// issue/related_issue/relation_type fields, the remaining facts
    /// from each row's `why`, and `included` is the oracle.
    #[test]
    fn edge_rows_verdicts() {
        let rows = fixture("edges.rows.json");
        assert_eq!(rows["dependent"].as_str(), Some("FX3A-1"));
        let edges = rows["edges"].as_array().expect("edges is array");
        assert_eq!(edges.len(), 14);
        let dependent = rows["dependent"].as_str().unwrap();
        let mut seen = Vec::new();
        for edge in edges {
            let label = edge["label"].as_str().expect("label is str");
            let issue = edge["issue"].as_str().unwrap();
            let related = edge["related_issue"].as_str().unwrap();
            let rel_type = edge["relation_type"].as_str().unwrap();
            // Orientation from the fixture fields: forward rows anchor
            // issue=D under blocked_by, stored-reversed rows anchor
            // related=D under blocking.
            let oriented = (issue == dependent && rel_type == BLOCKED_BY)
                || (related == dependent && rel_type == BLOCKING)
                || issue == related; // self-edge row carries either shape
            assert!(oriented, "{label} is a blocker-oriented row");
            let facts = EdgeFacts {
                relation_live: edge["deleted"].is_null(),
                self_edge: issue == related,
                target_live: !matches!(
                    label,
                    "archived-target" | "draft-target" | "triage-target" | "deleted-target"
                ),
                target_same_workspace: label != "x-workspace",
            };
            assert_eq!(
                edge_included(&facts),
                edge["included"].as_bool().expect("included is bool"),
                "{label}: {}",
                edge["why"].as_str().unwrap_or("?"),
            );
            seen.push(label);
        }
        seen.sort_unstable();
        assert_eq!(
            seen,
            [
                "archived-target",
                "cross-project",
                "deleted-target",
                "draft-target",
                "fwd-open",
                "fwd-resolved",
                "no-state-open",
                "self-edge",
                "soft-deleted",
                "stored-reversed-open",
                "stored-reversed-resolved",
                "test-state-open",
                "triage-target",
                "x-workspace",
            ]
        );
    }

    /// Row and bulk forms share the edge builders: the generated
    /// statements literally contain the builders' output, so they
    /// cannot disagree about what counts as a blocker.
    #[test]
    fn statements_share_edge_builders() {
        let live = live_targets_sql();
        let (fwd, rev) = blocked_by_edges_sql(":issue_id", &live);
        for sql in [blockers_sql(), open_blockers_sql(), has_open_blockers_sql()] {
            assert!(sql.contains(&fwd), "row form carries forward edges");
            assert!(sql.contains(&rev), "row form carries stored-reversed edges");
        }
        let (dep_fwd, dep_rev) = blocking_edges_sql(":issue_id", &live);
        assert!(dependents_sql().contains(&dep_fwd));
        assert!(dependents_sql().contains(&dep_rev));
        assert!(summary_sql(false).contains(&fwd));
        assert!(summary_sql(false).contains(&rev));
        assert!(summary_sql(true).contains(&dep_fwd));
        assert!(summary_sql(true).contains(&dep_rev));
        // The bulk predicate is the same forward pair over the open
        // targets with an outer-column anchor.
        let open = open_targets_sql();
        let (bulk_fwd, bulk_rev) = blocked_by_edges_sql(OPEN_BLOCKERS_Q_DEFAULT_OUTER, &open);
        let bulk = open_blockers_q_sql();
        assert!(bulk.contains(&bulk_fwd));
        assert!(bulk.contains(&bulk_rev));
        // ... and the only difference between the row and bulk edge
        // text is the anchor expression plus the open-targets filter.
        assert!(bulk_fwd.contains(OPEN_BLOCKERS_Q_DEFAULT_OUTER));
        assert!(!fwd.contains(OPEN_BLOCKERS_Q_DEFAULT_OUTER));
    }

    /// Every `edges.sql` statement replays predicate-by-predicate:
    /// liveness, self-edge, anchor, types, targets, workspace, union,
    /// order — plus the open filter where the fixture has it.
    #[test]
    fn row_statements_match_edge_sql() {
        let edges = fixture("edges.sql");
        let cases = [
            ("blockers", blockers_sql(), false),
            ("open_blockers", open_blockers_sql(), true),
            ("has_open_blockers", has_open_blockers_sql(), true),
            ("dependents", dependents_sql(), false),
        ];
        for (key, ours, open) in cases {
            let statements = edges[key].as_array().expect("statement list");
            assert_eq!(statements.len(), 1, "{key} is one statement");
            let recorded = statements[0]["sql"].as_str().expect("sql is str");
            // Outer live-work-item scope (Issue.issue_objects).
            assert_same_predicate(
                recorded,
                &ours,
                "\"issues\".\"deleted_at\" IS NULL",
                "i.\"deleted_at\" IS NULL",
                &format!("{key} outer liveness"),
            );
            assert_same_predicate(
                recorded,
                &ours,
                "NOT (\"states\".\"group\" = 'triage' AND \"states\".\"group\" IS NOT NULL)",
                "NOT (s.\"group\" = 'triage' AND s.\"group\" IS NOT NULL)",
                &format!("{key} triage exclusion keeps NULL groups live"),
            );
            assert_same_predicate(
                recorded,
                &ours,
                "NOT (\"issues\".\"archived_at\" IS NOT NULL)",
                "NOT (i.\"archived_at\" IS NOT NULL)",
                &format!("{key} unarchived issue"),
            );
            assert_same_predicate(
                recorded,
                &ours,
                "NOT (\"projects\".\"archived_at\" IS NOT NULL)",
                "NOT (p.\"archived_at\" IS NOT NULL)",
                &format!("{key} unarchived project"),
            );
            assert_same_predicate(
                recorded,
                &ours,
                "NOT (\"issues\".\"is_draft\")",
                "NOT (i.\"is_draft\")",
                &format!("{key} non-draft"),
            );
            // Live relations minus self-edges.
            assert_same_predicate(
                recorded,
                &ours,
                "V0.\"deleted_at\" IS NULL AND NOT (V0.\"issue_id\" = (V0.\"related_issue_id\"))",
                "r.\"deleted_at\" IS NULL AND NOT (r.\"issue_id\" = r.\"related_issue_id\")",
                &format!("{key} live relations"),
            );
            // Both relation types, one subquery each.
            let norm = normalize_sql(&ours);
            assert_eq!(norm.matches("r.relation_type = 'blocked_by'").count(), 1);
            assert_eq!(norm.matches("r.relation_type = 'blocking'").count(), 1);
            // Workspace match on the joined target.
            assert_same_predicate(
                recorded,
                &ours,
                "V1.\"workspace_id\" = (V0.\"workspace_id\")",
                "t.\"workspace_id\" = r.\"workspace_id\"",
                &format!("{key} workspace match"),
            );
            // Targets drawn from the live-issues subquery.
            assert_same_predicate(
                recorded,
                &ours,
                "SELECT U0.\"id\" FROM \"issues\" U0",
                "SELECT lt.\"id\" FROM \"issues\" lt",
                &format!("{key} live targets"),
            );
            // The OR-union over the two edge directions.
            if key == "dependents" {
                assert_same_predicate(
                    recorded,
                    &ours,
                    "\"issues\".\"id\" IN (SELECT V0.\"issue_id\"",
                    "i.\"id\" IN (SELECT r.\"issue_id\"",
                    "dependents forward union",
                );
                assert_same_predicate(
                    recorded,
                    &ours,
                    "OR \"issues\".\"id\" IN (SELECT V0.\"related_issue_id\"",
                    "OR i.\"id\" IN (SELECT r.\"related_issue_id\"",
                    "dependents reversed union",
                );
            } else {
                assert_same_predicate(
                    recorded,
                    &ours,
                    "\"issues\".\"id\" IN (SELECT V0.\"related_issue_id\"",
                    "i.\"id\" IN (SELECT r.\"related_issue_id\"",
                    &format!("{key} forward union"),
                );
                assert_same_predicate(
                    recorded,
                    &ours,
                    "OR \"issues\".\"id\" IN (SELECT V0.\"issue_id\"",
                    "OR i.\"id\" IN (SELECT r.\"issue_id\"",
                    &format!("{key} reversed union"),
                );
            }
            // Open filter exactly where the fixture has it.
            let open_needle =
                "NOT (\"states\".\"group\" IN ('cancelled', 'completed') AND \"states\".\"group\" IS NOT NULL)";
            assert_eq!(
                normalize_sql(recorded).contains(&normalize_sql(open_needle)),
                open,
                "{key} fixture open filter"
            );
            assert_eq!(
                normalize_sql(&ours).contains(&normalize_sql(
                    "NOT (s.\"group\" IN ('cancelled', 'completed') AND s.\"group\" IS NOT NULL)"
                )),
                open,
                "{key} port open filter"
            );
        }
        // Statement shapes: full rows + `_ordered`, `SELECT 1 ... LIMIT 1`.
        assert_same_predicate(
            edges["blockers"][0]["sql"].as_str().unwrap(),
            &blockers_sql(),
            "ORDER BY \"projects\".\"identifier\" ASC, \"issues\".\"sequence_id\" ASC",
            "ORDER BY p.\"identifier\" ASC, i.\"sequence_id\" ASC",
            "blockers order",
        );
        let has_recorded = edges["has_open_blockers"][0]["sql"].as_str().unwrap();
        assert!(normalize_sql(has_recorded).starts_with("select 1 as a"));
        assert!(normalize_sql(has_recorded).ends_with("limit 1"));
        assert!(normalize_sql(&has_open_blockers_sql()).starts_with("select 1 as a"));
        assert!(normalize_sql(&has_open_blockers_sql()).ends_with("limit 1"));
        // Row statements project the documented columns in order.
        for sql in [blockers_sql(), open_blockers_sql(), dependents_sql()] {
            assert!(
                sql.starts_with(&format!("SELECT {BLOCKER_ROW_COLUMNS}")),
                "row projection: {sql}"
            );
        }
    }

    /// `open_blockers_q.sql`: `Exists` x 2 over the open targets with
    /// the default `pk` anchor — and the bulk scan flags exactly the
    /// candidates the row goldens call open.
    #[test]
    fn bulk_predicate_matches_exists_sql() {
        let bulk = fixture("open_blockers_q.sql");
        let statements = bulk["executed_sql"].as_array().expect("executed list");
        assert_eq!(statements.len(), 1);
        let recorded = statements[0]["sql"].as_str().expect("sql is str");
        let ours = open_blockers_q_sql();
        // Exists x 2, one per edge direction.
        assert_eq!(
            normalize_sql(recorded)
                .matches("exists(select 1 as a")
                .count(),
            2
        );
        assert_eq!(normalize_sql(&ours).matches("exists(select").count(), 2);
        assert_eq!(normalize_sql(&ours).matches(" or exists(").count(), 1);
        // Same forward + stored-reversed pair as the row form.
        assert_same_predicate(
            recorded,
            &ours,
            "V0.\"relation_type\" = 'blocked_by'",
            "r.\"relation_type\" = 'blocked_by'",
            "bulk forward type",
        );
        assert_same_predicate(
            recorded,
            &ours,
            "V0.\"relation_type\" = 'blocking'",
            "r.\"relation_type\" = 'blocking'",
            "bulk reversed type",
        );
        // Default anchor is the outer pk.
        assert_same_predicate(
            recorded,
            &ours,
            "V0.\"issue_id\" = (\"issues\".\"id\")",
            "r.\"issue_id\" = \"issues\".\"id\"",
            "bulk default anchor",
        );
        // Bulk targets are the *open* live issues.
        assert_same_predicate(
            recorded,
            &ours,
            "NOT (U1.\"group\" IN ('cancelled', 'completed') AND U1.\"group\" IS NOT NULL)",
            "NOT (ls.\"group\" IN ('cancelled', 'completed') AND ls.\"group\" IS NOT NULL)",
            "bulk open targets",
        );
        // Non-default anchor: the ticker form from the docstring.
        let ticker = open_blockers_q_sql_for("\"issue_agent_ticker\".\"issue_id\"");
        assert!(ticker.contains("\"issue_agent_ticker\".\"issue_id\""));
        assert!(!ticker.contains(":issue_id"));
        assert!(OPEN_BLOCKERS_Q_PARAMS.is_empty());
        // Scan consistency: flagged == {candidates whose row golden is open}.
        let scan = &bulk["scan"];
        let candidates: Vec<&str> = scan["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(candidates, ["FX3A-1", "FX3A-13", "FX3A-15", "FX3A-2"]);
        let flagged: Vec<&str> = scan["flagged"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(flagged, ["FX3A-1"]);
        let golden = fixture("blockers.golden.json");
        assert_eq!(golden["has_open_blockers_D"].as_bool(), Some(true)); // FX3A-1
        assert_eq!(
            golden["has_open_blockers_D2_all_resolved"].as_bool(),
            Some(false)
        ); // FX3A-13
        assert_eq!(
            golden["has_open_blockers_D3_no_relations"].as_bool(),
            Some(false)
        ); // FX3A-15
    }

    /// `blockers.golden.json` row goldens replay through
    /// [`order_rows`] + [`open_only`]: input order is scrambled first
    /// so the assertions pin the logic, not the input.
    #[test]
    fn row_goldens_replay() {
        let golden = fixture("blockers.golden.json");
        // blockers(D): 7 rows, ordered by project then sequence.
        let mut blockers = rows_from_golden(&golden["blockers_D"]);
        blockers.reverse();
        order_rows(&mut blockers);
        assert_eq!(
            row_identifiers(&blockers),
            ["FX3A-2", "FX3A-3", "FX3A-4", "FX3A-5", "FX3A-11", "FX3B-1", "FX3B-2"]
        );
        // open_blockers(D): the 5 open ones, order preserved.
        let open = open_only(&blockers);
        assert_eq!(
            row_identifiers(&open),
            ["FX3A-2", "FX3A-4", "FX3A-11", "FX3B-1", "FX3B-2"]
        );
        let expected_open: Vec<&str> = golden["open_blockers_D"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["identifier"].as_str().unwrap())
            .collect();
        assert_eq!(row_identifiers(&open), expected_open);
        // has_open_blockers agrees on D / all-resolved D2 / relation-less D3.
        assert_eq!(
            !open.is_empty(),
            golden["has_open_blockers_D"].as_bool().unwrap()
        );
        let d2 = rows_from_golden(&golden["blockers_D2"]);
        assert_eq!(row_identifiers(&d2), ["FX3A-14"]);
        assert!(open_only(&d2).is_empty());
        assert_eq!(golden["open_blockers_D2"].as_array().unwrap().len(), 0);
        assert_eq!(
            golden["has_open_blockers_D2_all_resolved"].as_bool(),
            Some(false)
        );
        assert_eq!(
            golden["has_open_blockers_D3_no_relations"].as_bool(),
            Some(false)
        );
        // dependents(D): any state, cross-project ordered.
        let mut dependents = rows_from_golden(&golden["dependents_D"]);
        dependents.reverse();
        order_rows(&mut dependents);
        assert_eq!(row_identifiers(&dependents), ["FX3A-12", "FX3B-3"]);
    }

    /// `relations_summary.golden.json` replays end to end through
    /// [`relations_summary`], including the null-state item and the
    /// Python key order in the rendered JSON.
    #[test]
    fn summary_golden_replays() {
        let fixture_value = fixture("relations_summary.golden.json");
        let golden = fixture("blockers.golden.json");
        let blocked_by = rows_from_golden(&golden["blockers_D"]);
        let blocking = rows_from_golden(&golden["dependents_D"]);
        let summary = relations_summary(
            &blocked_by,
            &blocking,
            golden["has_open_blockers_D"].as_bool().unwrap(),
        );
        let expected: RelationsSummary =
            serde_json::from_value(fixture_value["summary_D"].clone()).expect("summary shape");
        assert_eq!(summary, expected);
        // Open items first in both lists, resolved after.
        assert_eq!(
            identifiers(&summary.relations_summary.blocked_by),
            ["FX3A-2", "FX3A-4", "FX3A-11", "FX3B-1", "FX3B-2", "FX3A-3", "FX3A-5"]
        );
        assert_eq!(
            identifiers(&summary.relations_summary.blocking),
            ["FX3A-12", "FX3B-3"]
        );
        // Byte-level key order matches the Python dicts.
        let rendered = serde_json::to_string(&summary).unwrap();
        assert!(rendered.starts_with("{\"relations_summary\":{\"blocked_by\":["));
        assert!(rendered.contains("\"has_open_blockers\":true}"));
        assert!(
            rendered.contains("{\"identifier\":\"FX3A-11\",\"state\":null,\"state_group\":null}")
        );
    }

    /// The cap case: 101 blockers (1 open + 100 completed) yield a
    /// 100-item list headed by the open one while `has_open_blockers`
    /// stays true over the full set.
    #[test]
    fn summary_cap_case() {
        let fixture_value = fixture("relations_summary.golden.json");
        let cap = &fixture_value["cap_case"];
        let mut rows = vec![BlockerRow {
            issue_id: uuid::Uuid::from_u128(1),
            sequence_id: 17,
            project_identifier: "FX3A".to_owned(),
            state_name: Some("Todo".to_owned()),
            state_group: Some("unstarted".to_owned()),
        }];
        for (n, seq) in (18..=117).enumerate() {
            rows.push(BlockerRow {
                issue_id: uuid::Uuid::from_u128(n as u128 + 2),
                sequence_id: seq,
                project_identifier: "FX3A".to_owned(),
                state_name: Some("Done".to_owned()),
                state_group: Some("completed".to_owned()),
            });
        }
        assert_eq!(rows.len(), 101);
        rows.reverse(); // cap + open-first must not depend on input order
        let summary = relations_summary(&rows, &[], true);
        let list = &summary.relations_summary.blocked_by;
        assert_eq!(list.len() as u64, cap["blocked_by_len"].as_u64().unwrap());
        assert_eq!(list.len(), 100);
        assert_eq!(
            serde_json::to_value(&list[0]).unwrap(),
            cap["blocked_by_first"]
        );
        assert_eq!(
            serde_json::to_value(&list[99]).unwrap(),
            cap["blocked_by_last"]
        );
        let n_open = list
            .iter()
            .filter(|item| is_open_group(item.state_group.as_deref()))
            .count();
        assert_eq!(n_open as u64, cap["n_open_in_list"].as_u64().unwrap());
        assert_eq!(n_open, 1);
        assert_eq!(
            summary.has_open_blockers,
            cap["has_open_blockers"].as_bool().unwrap()
        );
        assert!(summary.relations_summary.blocking.is_empty());
        assert_eq!(cap["blocking"].as_array().unwrap().len(), 0);
    }

    /// `summary_D_sql_statements` is 3: the two directional summary
    /// statements plus the full-set open check.
    #[test]
    fn summary_runs_three_statements() {
        let fixture_value = fixture("relations_summary.golden.json");
        assert_eq!(fixture_value["summary_D_sql_statements"].as_u64(), Some(3));
        let blocked_by = summary_sql(false);
        let blocking = summary_sql(true);
        assert_ne!(blocked_by, blocking);
        for sql in [&blocked_by, &blocking] {
            assert!(sql.contains("CASE WHEN s.\"group\" IN ('cancelled', 'completed')"));
            assert!(sql.contains("ORDER BY \"_resolved\", p.\"identifier\", i.\"sequence_id\""));
            assert!(sql.ends_with("LIMIT 100"));
            assert!(sql.contains(":issue_id"));
        }
        assert_eq!(ISSUE_PARAMS, &["issue_id"]);
        assert!(normalize_sql(&has_open_blockers_sql()).ends_with("limit 1"));
    }

    /// `None` renders as an explicit `null` — never an absent key.
    #[test]
    fn none_renders_explicit_null_keys() {
        let row = BlockerRow {
            issue_id: uuid::Uuid::nil(),
            sequence_id: 11,
            project_identifier: "FX3A".to_owned(),
            state_name: None,
            state_group: None,
        };
        let value = serde_json::to_value(summary_item(&row)).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(object.len(), 3);
        assert_eq!(object["identifier"].as_str(), Some("FX3A-11"));
        assert!(object["state"].is_null());
        assert!(object["state_group"].is_null());
    }
}
