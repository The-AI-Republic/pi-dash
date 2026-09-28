//! Space public-API handlers (D-02, stage 4).
//!
//! Axum shell over `pidash_services::space`: query builders from
//! [`project_meta`](pidash_services::space::queries::project_meta),
//! anchor/error mapping from
//! [`guards`](pidash_services::space::guards), and the project-lite leaf
//! from [`lite`](pidash_services::space::serializers::lite).
//!
//! [`routes`] registers the nine project/meta/taxonomy GETs; every other
//! method on those paths falls through to Django through the cutover edge
//! (route registration is the cutover granularity, no flag needed).

pub mod project_meta;

pub use project_meta::routes;
