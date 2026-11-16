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
//!
//! [`handlers_misc`] owns handler units C (PIDASHCONV-357): date-check,
//! transfer and user-properties, with the request-context plumbing
//! (`Actor`, `Tenant`, `Membership`, `Denial`) sibling handler issues
//! reuse through it.

pub mod gates;
pub mod handlers_analytics;
pub mod handlers_archive;
pub mod handlers_cycle_issues;
pub mod handlers_cycles;
pub mod handlers_favorites;
pub mod handlers_misc;
pub mod handlers_progress;

use axum::Router;

use crate::state::AppState;

/// Merge the app-cycles route groups (archive first, PIDASHCONV-377;
/// cycle-issues + favorites via PIDASHCONV-323; progress + analytics via
/// PIDASHCONV-410; CycleViewSet CRUD via PIDASHCONV-321; date-check +
/// transfer + user-properties via PIDASHCONV-357; sibling handler issues
/// extend the merge; merges keep
/// both sides). Cutover into the serving router stays with the domain
/// gate (PIDASHCONV-388), so this is additive only.
pub fn routes() -> Router<AppState> {
    handlers_archive::routes()
        .merge(handlers_cycle_issues::routes())
        .merge(handlers_favorites::routes())
        .merge(handlers_progress::routes())
        .merge(handlers_analytics::routes())
        .merge(handlers_misc::routes())
        .merge(handlers_cycles::routes())
}

pub use pidash_services::app_cycles::queries;

/// Re-exported `IssueFilterSet` surface for the cycle-issue endpoint
/// (`app/views/cycle/issue.py:49`).
pub use queries::CYCLE_ISSUE_FILTERSET_FIELDS;

/// Django's `<uuid:>` converter is lowercase-hyphenated only
/// (`django/urls/converters.py`, compiled case-sensitively). Every other
/// spelling `Uuid::parse_str` accepts — uppercase, simple 32-hex, braced
/// `{…}`, `urn:uuid:…` — fails URL resolution and answers Django's
/// resolver 404 before any auth or view code runs. Mirrors the
/// PIDASHCONV-1132 `license` helper (`license/handlers_admin.rs`).
pub(crate) fn is_canonical_uuid(raw: &str) -> bool {
    if raw.len() != 36 {
        return false;
    }
    raw.bytes().enumerate().all(|(index, byte)| {
        if index == 8 || index == 13 || index == 18 || index == 23 {
            byte == b'-'
        } else {
            byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
        }
    })
}

/// A path segment `Uuid::parse_str` accepts but Django's `<uuid:>` never
/// routes (uppercase / simple / braced / urn). Such routes must proxy to
/// Django so its own resolver 404 comes back byte-for-byte, rather than
/// the segment being served like the canonical id.
pub(crate) fn is_noncanonical_uuid(raw: &str) -> bool {
    raw.parse::<uuid::Uuid>().is_ok() && !is_canonical_uuid(raw)
}

/// Rebuild the incoming request (method, uri and headers only, empty
/// body) and proxy it to Django. The resolver 404 reads no body, and the
/// PIDASHCONV-1095/-1132 precedents rebuild exactly this way.
pub(crate) async fn proxy_segment(
    state: AppState,
    method: axum::http::Method,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    let mut builder = axum::http::Request::builder().method(method).uri(uri);
    for (name, value) in headers.iter() {
        // The body is rebuilt empty, so a copied `Content-Length` (or a
        // `Transfer-Encoding`) would describe bytes that never arrive and
        // hang Django waiting for them. Let the client set the length.
        if name == axum::http::header::CONTENT_LENGTH
            || name == axum::http::header::TRANSFER_ENCODING
        {
            continue;
        }
        builder = builder.header(name, value);
    }
    let req = builder
        .body(axum::body::Body::empty())
        .expect("proxy request");
    crate::edge::proxy(axum::extract::State(state), req).await
}

/// Endpoints whose list path runs the shared `ComplexFilterBackend`
/// JSON-tree filter before the filterset (`issue.py:43` + `:119`).
pub const COMPLEX_FILTER_ENDPOINTS: &[&str] = &["cycle-issues"];

/// Endpoints whose list path runs the legacy `issue_filters(params,
/// 'GET')` dict compiler first (`issue.py:111`).
pub const LEGACY_FILTER_ENDPOINTS: &[&str] = &["cycle-issues"];

/// `base_host(request, is_app=True)` (`utils/host.py:17-67`): the app
/// origin is `APP_BASE_URL` when truthy, else `WEB_URL` when truthy —
/// `None` (the caller's 500, Django's `ImproperlyConfigured`) when both
/// are unset. Empty strings count as unset, matching Python truthiness.
///
/// One slice-local helper for the five activity-enqueue sites (cycle
/// create/PATCH/destroy, transfer, cycle-issue create/destroy).
pub(crate) fn activity_origin(state: &AppState) -> Option<String> {
    let urls = &state.settings().urls;
    urls.app_base_url
        .clone()
        .filter(|value| !value.is_empty())
        .or_else(|| urls.web_url.clone().filter(|value| !value.is_empty()))
}

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
