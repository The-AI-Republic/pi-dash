//! App views + search services (D-29, stage 5).
//!
//! * [`serializers`] — the four serializer units owned by PIDASHCONV-269:
//!   `IssueViewSerializer`, `ViewIssueListSerializer`, `ViewFavoriteSerializer`
//!   and `UserFavoriteSerializer`.
//! * [`queries_views`] — the five view-queryset query units owned by
//!   PIDASHCONV-271: workspace / project / favorite querysets, the
//!   view-issues queryset with annotations and permission filters, and the
//!   view-issues list pipeline.
pub mod queries_views;
pub mod serializers;
