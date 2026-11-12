#![forbid(unsafe_code)]

//! api-v1 assets / stickies / intake layer (D-21, stage 5).
//!
//! Ports the `permission_classes` / authentication / throttle lines for the
//! api-v1 surface (see [`permissions`] for the gates) plus the five
//! handler units per endpoint family: [`asset_user`], [`asset_server`],
//! [`asset_generic`], [`sticky`], and [`intake`].
//!
//! [`routes`] aggregates every handler module's router; the `ApiV1`
//! overlay wiring belongs to PIDASHCONV-426 (the intake issue) — no other
//! D-21 issue touches `overlay.rs`/`routes.rs`.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod asset_generic;
pub mod asset_server;
pub mod asset_user;
pub mod intake;
pub mod permissions;
pub mod sticky;

/// The whole `v1_assets` surface: user assets, server assets, generic
/// assets, stickies, and intake issues. The ten paths are pairwise
/// disjoint, so the merge cannot panic; every unported method on those
/// paths proxies to Django from inside each module's `routes()`.
pub fn routes() -> axum::Router<crate::state::AppState> {
    asset_user::routes()
        .merge(asset_server::routes())
        .merge(asset_generic::routes())
        .merge(sticky::routes())
        .merge(intake::routes())
}
