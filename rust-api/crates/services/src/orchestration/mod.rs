#![forbid(unsafe_code)]

//! Orchestration engine services (D-12, stage 5).
//!
//! Ports the service layer of the orchestration engine:
//!
//! * [`blockers`] — `orchestration/blockers.py` whole (blocker
//!   lookups: edge kernels, row form, bulk `Exists` predicate,
//!   relations summary).
//!
//! Read-only queries: this module opens no cross-domain dependency
//! and no D-11 edge (early-release rule). SQL here is text plus
//! `:name` placeholders per the services `queries.rs` precedent
//! (D-27/D-30/D-36); handlers translate each `:name` to a positional
//! `$n` in `*_PARAMS` order. The closed-group set reuses
//! [`pidash_db::app_project::models::state::StateGroup`] — never
//! re-ported.
//!
//! Fixture id replayed by the unit tests alongside [`blockers`]:
//! FX-ORCH-03 (`rust-api/fixtures/orchestration/fx03_blockers/`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod blockers;

pub use blockers::{
    blocked_by_edges_sql, blockers_sql, blocking_edges_sql, dependents_sql, edge_included,
    has_open_blockers_sql, is_closed_group, is_open_group, is_open_row, live_issue_predicate,
    live_relation_predicate, live_targets_sql, open_blockers_q_sql, open_blockers_q_sql_for,
    open_blockers_sql, open_only, open_predicate, open_targets_sql, order_rows, relations_summary,
    resolved_case_sql, summary_item, summary_list, summary_sql, BlockerRow, EdgeFacts,
    RelationDirections, RelationsSummary, SummaryItem, BLOCKED_BY, BLOCKER_ROW_COLUMNS, BLOCKING,
    CLOSED_STATE_GROUPS, ISSUE_PARAMS, OPEN_BLOCKERS_Q_DEFAULT_OUTER, OPEN_BLOCKERS_Q_PARAMS,
    ORDER_BY, PLACEHOLDER_RULE, SUMMARY_LIMIT, SUMMARY_ORDER_BY,
};
