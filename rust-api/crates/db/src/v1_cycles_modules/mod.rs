#![forbid(unsafe_code)]

//! api-v1 cycles + modules domain surface, db layer (D-20, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/module.py` for the db layer:
//!
//! * [`module`] — `Module`, `ModuleMember`, `ModuleIssue`, `ModuleLink`,
//!   `ModuleUserProperties` + `ModuleStatus` (PIDASHCONV-292: column lists,
//!   manager exclusion filters, `archived_at` guards, per-module scoping,
//!   save rules).
//! * [`module_queries`] — the five module read querysets M1-M5
//!   (PIDASHCONV-308: tenant scopes, archived filters, issue-count
//!   annotations, member-visibility asymmetry, kwargs/GET ordering).
//!   Serializers, guards, tasks and handlers belong to the sibling D-20
//!   issues; the domain gate is PIDASHCONV-425.
//!
//! Wiring note: the crate root declares `pub mod v1_cycles_modules;`
//! (seam added by PIDASHCONV-292); every file under this module is new.
//! Sibling issues add their own files under this module (e.g.
//! PIDASHCONV-291 `cycle`, PIDASHCONV-307 `cycle_queries`); on rebase
//! keep both sides.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod module;
pub mod module_queries;
