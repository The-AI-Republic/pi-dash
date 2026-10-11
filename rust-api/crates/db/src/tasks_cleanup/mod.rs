//! D-09 cleanup retention + mongo flush queries.
//!
//! Port of the driver and queryset layer of
//! `apps/api/pi_dash/bgtasks/cleanup_task.py` (`:38-:163`, `:267-:421`).
//!
//! Background-task query surface for the D-09 cleanup domain (stage 5).
//!
//! Ports the data-access half of `apps/api/pi_dash/bgtasks/deletion_task.py`
//! (PIDASHCONV-186). The walk orchestration lives in `pidash-services`
//! (`tasks_cleanup::deletion`); the worker wiring lives in `pidash-jobs`
//! (`tasks_cleanup::deletion`).

pub mod cleanup_queries;
pub mod deletion_queries;

pub use deletion_queries::{
    cascade_collected_children_sql, cascade_seed_children_sql, classify_delete_edges, cutoff_for,
    delete_ids_sql, entry_table_for, fetch_live_to_many, fetch_one_to_one, fetch_row,
    format_cutoff, hard_delete_before_sql, hard_delete_seed_exists_sql, load_fk_details,
    load_schema_info, now_stamp, null_bulk, null_fk_bulk_sql, null_fk_emulation_ids_sql,
    null_fk_emulation_seed_sql, null_fk_one_to_one_sql, null_one_to_one, parse_pk,
    protect_collected_check_sql, protect_seed_check_sql, reverse_relations, run_model_hard_delete,
    soft_stamp_sql, stamp_row, sweep_tables_in_django_order, FkEdge, LookupError, ModelDeleteError,
    ModelDeleteOutcome, ReverseRelation, RowPk, RowState, SchemaInfo, HARD_DELETE_NAMED_TABLES,
    HARD_DELETE_SWEEP_TABLES,
};

// D-09 version-task query units (stage 5, PIDASHCONV-188): the SQL behind
// `issue_version_sync.py`, `issue_description_version_sync.py`,
// `issue_description_version_task.py` and `page_version_task.py`.
pub mod version_queries;
