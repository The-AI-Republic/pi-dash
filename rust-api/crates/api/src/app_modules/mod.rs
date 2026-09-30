//! D-28 app-modules permission layer (stage 5, PIDASHCONV-379).
//!
//! [`gates`] ports the guard closure on all 13 D-28 routes: the
//! `@allow_permission` role sets, the `ProjectEntityPermission` vs
//! `ProjectLitePermission` branch mapping, and the per-user scoping notes.
//! The module is pure: handlers fetch membership rows through the
//! workspace-scoped handle and decide here. Sibling handler issues own
//! the HTTP shell; merges keep both sides.

pub mod gates;
