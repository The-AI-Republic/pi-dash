#![forbid(unsafe_code)]

//! HTTP layer: axum routers, extractors, and middleware.
//!
//! Routers here mirror Django's URL paths exactly; per-domain routers live in
//! `src/<domain>/` and are merged into [`build_router`]. The serializer and
//! paginator kernel arrive under F-07, middleware under F-08.

pub mod edge;
pub mod routes;
pub mod state;

pub use edge::{EdgeFlags, EdgeHandle, Prefix, DEFAULT_UPSTREAM};
pub use routes::{build_router, with_routes};
pub use state::AppState;
