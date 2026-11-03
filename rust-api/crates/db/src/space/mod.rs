//! Space public API (`api/public/`, D-02) read model.
//!
//! Column lists and manager read-scopes for every table the space views
//! touch. Django stays the schema owner: this module emits no DDL and
//! changes no schema; it records the exact column sets (defaults,
//! `db_table`, ordering, `unique_together`) and the manager `WHERE`
//! predicates the queries layer (PIDASHCONV-167…171) applies.

pub mod columns;
