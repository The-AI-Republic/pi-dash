#![forbid(unsafe_code)]

//! Orchestration engine services (D-12, stage 5).
//!
//! Ports the service layer of the orchestration engine:
//!
//! * [`blockers`] — `orchestration/blockers.py` whole (blocker
//!   lookups: edge kernels, row form, bulk `Exists` predicate,
//!   relations summary).
//! * [`relations`] — `orchestration/relations.py` whole (relation
//!   vocabulary, ref resolution, idempotent relate/unrelate writes,
//!   grouped list, activity enqueue).
//! * [`clock`] — `orchestration/scheduling.py` clock core (`reconcile`,
//!   the five event handlers, clock primitives, thin senders).
//!
//! Read-only queries ([`blockers`]) plus handler-executed write SQL
//! ([`relations`]: relate `INSERT`, unrelate soft-delete `UPDATE`,
//! activity emits; [`clock`]: ticker lock `SELECT ... FOR UPDATE`, create
//! `INSERT`, clock-field `UPDATE`): this module opens no cross-domain
//! dependency and no D-11 edge (early-release rule). SQL here is text plus
//! `:name` placeholders per the services `queries.rs` precedent
//! (D-27/D-30/D-36); handlers translate each `:name` to a positional
//! `$n` in `*_PARAMS` order. The closed-group set reuses
//! [`pidash_db::app_project::models::state::StateGroup`] — never
//! re-ported — and the stored/inverse mapping reuses the merged
//! assistant `actual_relation` / `inverse_relation` helpers, plus
//! `py_strip` for `strip()` parity; [`clock`] additionally reuses the L1
//! event/decision/outcome/phase types, `prompting::recipes::kind_for`, the
//! `db::tasks_ticker` row helpers and `db::dispatch::AgentRunStatus` (the
//! only intra-crate calls here; no D-11 edge).
//!
//! Fixture ids replayed by the unit tests: FX-ORCH-03
//! (`rust-api/fixtures/orchestration/fx03_blockers/`) alongside
//! [`blockers`], FX-ORCH-04
//! (`rust-api/fixtures/orchestration/fx04_relations/`) alongside
//! [`relations`], FX-ORCH-05
//! (`rust-api/fixtures/orchestration/fx05_clock/`) alongside [`clock`].
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod blockers;
pub mod clock;
pub mod relations;

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

pub use clock::{
    arm_ticker, clear_pending, clock_allowed, compute_next_run_at, create_ticker_sql,
    creates_ticker, disarm_ticker, effective_interval_seconds, is_paused_state, lock_ticker_sql,
    maybe_disarm_on_terminal_signal, new_ticker_row, on_enter_or_move, on_human_run_requested,
    on_left_bucket, on_retick, on_run_ended, pool_size, project_ticking_enabled, queue_entry,
    reconcile, reconcile_log_line, reset_ticker_after_comment_and_run, retime_clock, stop_clock,
    stop_for_switch, ClockError, ClockIssue, ClockOutcome, ClockWrite, ProjectClockPolicy,
    RunEndedRef, CREATE_TICKER_PARAMS, LOCK_TICKER_PARAMS, SAVE_CLOCK_PARAMS, SAVE_CLOCK_SQL,
    TICKER_CLOCK_FIELDS,
};

pub use relations::{
    check_targets, classify_ref, grouped_item, grouped_relations, identifier, pair_rows_sql,
    relate_created_emit, relate_created_requested_data, relate_insert_row, relate_insert_sql,
    relate_verdict, relation_current_instance, resolve_id_predicate, resolve_identifier_predicate,
    stored_edge, type_from, unrelate_delete_sql, unrelate_deleted_emit, unrelate_matches,
    unrelate_requested_data, unresolved_passthrough, validate_relation_type, ClassifiedRef,
    GroupedIssue, GroupedItem, GroupedRelations, PairRow, RefKind, RelateConflict, RelateInsert,
    RelateResult, RelateVerdict, RelationActivityEmit, RelationError, ResolvedIssue,
    UnrelateResult, GROUPED_RELATIONS_PARAMS, GROUP_LIMIT, ISSUE_ACTIVITY_KWARG_ORDER,
    ISSUE_ACTIVITY_TASK, PAIR_ROWS_PARAMS, RELATE_INSERT_PARAMS, RELATION_CREATED_ACTIVITY,
    RELATION_DELETED_ACTIVITY, RELATION_TYPES, RESOLVE_IDENTIFIER_PARAMS, RESOLVE_ID_PARAMS,
    REVERSE_TYPES, UNRELATE_DELETE_PARAMS,
};
