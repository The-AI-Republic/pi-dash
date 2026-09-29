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
//! * [`permissions`] — the five guard units owned by PIDASHCONV-273:
//!   workspace / project view gates, the view-issues list gate, the
//!   favorite gates and the search gates, over the F-06 kernel semantics.
pub mod fts;
pub mod permissions;
pub mod queries_search;
pub mod queries_views;
pub mod serializers;
