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
    cascade_to_many_message, cutoff_for, entry_table_for, fetch_live_one_to_one, fetch_row,
    format_cutoff, hard_delete_before_sql, hard_delete_table, load_schema_info, now_stamp,
    null_bulk, null_fk_bulk_sql, null_fk_one_to_one_sql, null_one_to_one, parse_pk,
    reverse_relations, run_named_hard_deletes, run_sweep_hard_deletes, soft_stamp_sql, stamp_row,
    sweep_tables, LookupError, ReverseRelation, RowState, SchemaInfo, HARD_DELETE_NAMED_TABLES,
};

// D-09 version-task query units (stage 5, PIDASHCONV-188): the SQL behind
// `issue_version_sync.py`, `issue_description_version_sync.py`,
// `issue_description_version_task.py` and `page_version_task.py`.
pub mod version_queries;
