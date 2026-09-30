#![forbid(unsafe_code)]

//! App cycles query seam (D-27, stage 5).
//!
//! Re-exports the services-layer queryset builders
//! (`pidash_services::app_cycles::queries`) and records which filter
//! backends each cycle endpoint applies, so the handler issues build on
//! one contract:
//!
//! - `CycleViewSet` list: search + order backends over the annotated
//!   base queryset (`app/views/cycle/base.py:179-180`, F-C27-03).
//! - `CycleIssueViewSet`: `ComplexFilterBackend` + `IssueFilterSet`
//!   restricted to [`CYCLE_ISSUE_FILTERSET_FIELDS`]
//!   (`app/views/cycle/issue.py:43-49`, F-C27-05). Leaf resolution is the
//!   shared F-04 `pidash_db::filterset` kernel (pilot-2 `app_issues`
//!   pattern).
//! - `CycleFavoriteViewSet`: `filter_queryset` over the favorites scope
//!   (`app/views/cycle/base.py:562-570`, F-C27-10).
//!
//! Route registration and handlers belong to the handler issues
//! (PIDASHCONV-321/323/357/377/410); this module is the query seam they
//! wire through, not a stub — it carries the backend-parity consts plus
//! the re-export, both pinned by the test below.
//!
//! [`gates`] ports the `@allow_permission` role matrix (F-C27-07,
//! PIDASHCONV-290) over the F-06 kernel for the same handlers.
//!
//! [`handlers_archive`] ports `CycleArchiveUnarchiveEndpoint`
//! (PIDASHCONV-377); [`routes`] merges its routes — sibling handler
//! issues extend the merge; merges keep both sides.

pub mod gates;
pub mod handlers_analytics;
pub mod handlers_archive;
pub mod handlers_progress;

use axum::Router;

use crate::state::AppState;

/// Merge the app-cycles route groups (archive first, PIDASHCONV-377;
/// progress + analytics via PIDASHCONV-410; sibling handler issues
/// extend the merge; merges keep both sides). Cutover into the serving
/// router stays with the domain gate (PIDASHCONV-388), so this is
/// additive only.
pub fn routes() -> Router<AppState> {
    handlers_archive::routes()
        .merge(handlers_progress::routes())
        .merge(handlers_analytics::routes())
}

pub use pidash_services::app_cycles::queries;

/// Re-exported `IssueFilterSet` surface for the cycle-issue endpoint
/// (`app/views/cycle/issue.py:49`).
pub use queries::CYCLE_ISSUE_FILTERSET_FIELDS;

/// Endpoints whose list path runs the shared `ComplexFilterBackend`
/// JSON-tree filter before the filterset (`issue.py:43` + `:119`).
pub const COMPLEX_FILTER_ENDPOINTS: &[&str] = &["cycle-issues"];

/// Endpoints whose list path runs the legacy `issue_filters(params,
/// 'GET')` dict compiler first (`issue.py:111`).
pub const LEGACY_FILTER_ENDPOINTS: &[&str] = &["cycle-issues"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seam_reexports_resolve_and_backends_cover_cycle_issues() {
        // The re-export is the seam: handlers call through this module.
        assert_eq!(
            CYCLE_ISSUE_FILTERSET_FIELDS,
            queries::CYCLE_ISSUE_FILTERSET_FIELDS
        );
        assert!(COMPLEX_FILTER_ENDPOINTS.contains(&"cycle-issues"));
        assert!(LEGACY_FILTER_ENDPOINTS.contains(&"cycle-issues"));
        // The base/archive/favorites lists use search+order backends, not
        // the complex tree — they must not appear here.
        assert!(!COMPLEX_FILTER_ENDPOINTS.contains(&"cycles"));
        assert!(!COMPLEX_FILTER_ENDPOINTS.contains(&"archived-cycles"));
        assert!(!COMPLEX_FILTER_ENDPOINTS.contains(&"user-favorite-cycles"));
    }
}
