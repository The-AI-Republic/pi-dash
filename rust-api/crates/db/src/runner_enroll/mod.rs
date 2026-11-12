//! Runner enrollment/auth/machine read model (D-13).
//!
//! Column lists and model-level pure methods for every table D-13 owns.
//! Django stays the schema owner: this module emits no DDL and changes
//! no schema; it records the exact column sets (defaults, `db_table`,
//! ordering, constraints, indexes), the manager `WHERE` predicates, and
//! the model-method cores (save denorm/auto-resolve, heartbeat, revoke)
//! the queries layer (PIDASHCONV-582…584) and services layer apply.

pub mod columns;
