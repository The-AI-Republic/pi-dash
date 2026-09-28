//! Space public-API read queries (D-02, stage 4).
//!
//! Ports the read/write query shapes Django emits for the space `views/`.
//! Each builder returns the SQL text (Postgres `$N` placeholders where
//! Django renders `%s`, per the `db/src/license/queries.rs` precedent) and
//! the row structs carry the exact wire key order of the matching
//! `.values(...)` list, so `serde_json::to_string` is byte-identical to the
//! fixture rows. Execution belongs to the handlers layer (PIDASHCONV-174),
//! which binds the documented `$N` params in order and maps row counts onto
//! the `get()` contract (`0 -> DoesNotExist`, `>1 -> MultipleObjectsReturned`).
//!
//! * [`project_meta`] — project/meta/cycle/module/state/label reads
//!   (`space/views/{project,meta,cycle,module,state,label}.py`).
//! * [`intake_assets`] — intake-issue + file-asset read/write queries
//!   (`space/views/{intake,asset}.py`).
//! * [`social`] — comment / issue-reaction / comment-reaction / vote reads
//!   (`space/views/issue.py`).
//!
//! Later layer issues extend this module (`issue_list` for PIDASHCONV-168);
//! they add files here, never a fork.
pub mod intake_assets;
pub mod project_meta;
pub mod social;
