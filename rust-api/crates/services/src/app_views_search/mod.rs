//! App views + search services (D-29, stage 5).
//!
//! * [`serializers`] — the four serializer units owned by PIDASHCONV-269:
//!   `IssueViewSerializer`, `ViewIssueListSerializer`, `ViewFavoriteSerializer`
//!   and `UserFavoriteSerializer`.
//! * [`queries_views`] — the five view-queryset query units owned by
//!   PIDASHCONV-271: workspace / project / favorite querysets, the
//!   view-issues queryset with annotations and permission filters, and the
//!   view-issues list pipeline.
//! * [`fts`] — the issue full-text search closure (`search/issue.py`,
//!   PIDASHCONV-272): vectors, websearch query, filter OR-chain, rank,
//!   headline, snippet.
//! * [`queries_search`] — the search query builders (`app/views/search/`,
//!   PIDASHCONV-272): global sections, entity branches, issue pipeline.
pub mod fts;
pub mod queries_search;
pub mod queries_views;
pub mod serializers;
