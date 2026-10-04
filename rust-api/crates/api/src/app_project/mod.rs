#![forbid(unsafe_code)]

//! App project / states / estimates seam (D-25, stage 5).
//!
//! [`gates`] ports the L7 guard layer (FX-APROJ-07, PIDASHCONV-569):
//! the `@allow_permission` route matrix, the `ProjectMemberPermission` /
//! `ProjectEntityPermission` / `WorkspaceUserPermission` classes, the
//! `can_mutate_states` state-write rule, and the `invalidate_cache` key
//! semantics — all pure, over the read-only F-06 kernel in
//! `pidash_auth::permissions`.
//!
//! Route registration and handlers belong to the handler issues
//! (PIDASHCONV-571/572/573/574); they consume this module's gates,
//! tenant context, denial bodies, and cache-key helpers. Sibling layer
//! issues extend this module; merges keep both sides.
//!
//! [`handlers_invites`] ports the invite / join / favorite / deploy-board
//! handler family (PIDASHCONV-573); [`routes`] merges its routes —
//! sibling handler issues extend the merge; merges keep both sides.
//!
//! [`handlers_workflow`] ports the state + estimate handler family
//! (`StateViewSet`, `IntakeStateEndpoint`,
//! `ProjectEstimatePointEndpoint`, `BulkEstimatePointEndpoint`,
//! `EstimatePointEndpoint`, FX-APROJ-10, PIDASHCONV-574); [`routes`]
//! merges its routes — sibling handler issues extend the merge; merges
//! keep both sides.
//!
//! Cutover into the serving router stays with the domain gate
//! (PIDASHCONV-575), so [`routes`] is additive only.

pub mod gates;
pub mod handlers_invites;
pub mod handlers_workflow;

use axum::Router;

use crate::state::AppState;

/// App project routes: each handler file exposes its own `routes()`
/// (sibling D-25 handler issues extend this merge; merges keep both
/// sides), merged here for the F-10 overlay seam.
pub fn routes() -> Router<AppState> {
    handlers_invites::routes().merge(handlers_workflow::routes())
}
