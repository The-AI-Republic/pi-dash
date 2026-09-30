//! D-35 app-analytics permission layer (stage 5, PIDASHCONV-358).
//!
//! [`gates`] ports the `@allow_permission` gates on all 14 D-35 routes
//! plus the `WorkSpaceAdminPermission` class on the analytic-view viewset.
//! The module is pure: handlers fetch membership rows through the
//! workspace-scoped handle and decide here. Sibling handler issues own
//! the HTTP shell; merges keep both sides.

pub mod gates;
