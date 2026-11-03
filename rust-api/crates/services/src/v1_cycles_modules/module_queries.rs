//! Module read orchestration over caller-supplied facts (D-20, stage 5).
//!
//! Ports the read-choice logic around the five module querysets in
//! `apps/api/pi_dash/api/views/module.py` (drift baseline `01a93e17`):
//! which archived filter each endpoint applies, which order default it
//! carries, which shapes need the acting user, and — for the issue
//! paths — whether the wire serves the `get_queryset` shape or the `GET`
//! inline shape. The SQL itself lives in the db sibling
//! (`pidash_db::v1_cycles_modules::module_queries`); this module decides
//! *which* builder the handler calls and with what inputs, composed over
//! facts the handler already loaded (URL kwargs, query params, actor).
//!
//! Fixture oracle: FX-CYCMOD-05 (same `module.sql` M1-M5 the db sibling
//! pins). The tests below replay the endpoint → shape decision table,
//! not the SQL.
//!
//! # The two orderings
//!
//! Queryset chains read `.order_by(self.kwargs.get("order_by",
//! "-created_at"))` (`module.py:170,373,567,773,981`) — a URL-kwargs
//! passthrough defaulting to descending. The M4/M5 `GET` inline chains
//! instead read `request.GET.get("order_by", "created_at")` (`:600,806`)
//! — a real query param defaulting to **ascending**. [`parse_order`]
//! serves both call sites with their own default; the leading `-`
//! selects descending exactly like Django.
//!
//! # Which shape serves which method
//!
//! * List `GET` (`:264-276`) → M1 live ([`ReadShape::ModuleList`]).
//! * Detail `GET` (`:468-475`) → M2 ([`ReadShape::ModuleDetail`]).
//! * Archived-list `GET` (`:1006-1018`) → M3 ([`ReadShape::ArchivedModuleList`]).
//! * Issue-list `GET` (`:594-639`) → the M4 inline shape
//!   ([`ReadShape::ModuleIssueListGet`]); the issue-list `POST` re-read
//!   (`:730-733`) serves the M4 *queryset* shape instead
//!   ([`ReadShape::ModuleIssueQueryset`]).
//! * Issue-detail `GET` (`:800-849`) → [`ReadShape::ModuleIssueDetailGet`],
//!   which is unreachable on the wire: `api/urls/module.py:33-37` allows
//!   only `delete` on the detail route (ported bug 6, pinned by
//!   [`MODULE_ISSUE_DETAIL_ROUTED_METHODS`]).
//! * Issue-detail `DELETE` (`:864-888`) → [`ReadShape::ModuleIssueDelete`].
//!
//! Direct `.get()` call sites (detail `PATCH` `:408`, detail `DELETE`
//! `:496`, archive `POST` `:1040`, unarchive `DELETE` `:1074`, issue-list
//! `POST` module load `:671`) bypass every queryset — plain pk lookups
//! with the model's default-manager scope. They are handler-owned and
//! have no builder here; they are listed so the mapping reads total.
//!
//! Out of scope (sibling D-20 issues): envelopes and serializer field
//! selection (handlers, PIDASHCONV-406), `ProjectEntityPermission` gates
//! (PIDASHCONV-309), activity enqueues (PIDASHCONV-310), prefetch round
//! trips (handlers).

use pidash_db::v1_cycles_modules::module_queries::{ArchivedFilter, OrderBy};

/// A module read shape: which db builder the handler calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadShape {
    /// M1 list (`module.py:85-171` + `:272`).
    ModuleList,
    /// M2 detail (`module.py:288-374` + `:473`).
    ModuleDetail,
    /// M3 archived list (`module.py:895-982`).
    ArchivedModuleList,
    /// M4/M5 `get_queryset` (`module.py:545-569` / `:751-775`).
    ModuleIssueQueryset,
    /// M4 `GET` inline queryset (`module.py:601-634`).
    ModuleIssueListGet,
    /// M5 `GET` inline queryset (`module.py:807-843`; unrouted, see
    /// [`MODULE_ISSUE_DETAIL_ROUTED_METHODS`]).
    ModuleIssueDetailGet,
    /// M5 `DELETE` lookup (`module.py:870-875`).
    ModuleIssueDelete,
}

/// HTTP methods the `module-issues-detail` route actually wires
/// (`api/urls/module.py:33-37`, `http_method_names=["delete"]`). The
/// `ModuleIssueDetailAPIEndpoint.get` handler exists (`module.py:800`)
/// but no route reaches it (ported bug 6 — cycles wire `get` + `delete`
/// on the equivalent route).
pub const MODULE_ISSUE_DETAIL_ROUTED_METHODS: &[&str] = &["delete"];

/// Parse one `.order_by(...)` / `request.GET.get("order_by", ...)`
/// argument into the [`OrderBy`] the db builder quotes.
///
/// `raw` is the kwarg (`None` = key absent → `default`) or the query
/// param (`None` = param absent → `default`); `default` is `"-created_at"`
/// for the queryset chains and `"created_at"` for the M4/M5 GET chains.
/// A leading `-` selects descending, exactly like Django; the column
/// text (including an empty or unknown column) passes through untouched
/// and fails at the database like Django's `FieldError`-at-evaluation.
pub fn parse_order(raw: Option<&str>, default: &str) -> OrderBy {
    let text = raw.unwrap_or(default);
    match text.strip_prefix('-') {
        Some(column) => OrderBy::new(column, true),
        None => OrderBy::new(text, false),
    }
}

/// The queryset-chain order default (`module.py:170,373,567,773,981`).
pub const QUERYSET_ORDER_DEFAULT: &str = "-created_at";

/// The M4/M5 `GET` order default (`module.py:600,806`) — ascending,
/// unlike every queryset chain.
pub const ISSUE_GET_ORDER_DEFAULT: &str = "created_at";

/// The `archived_at` predicate a read shape applies
/// (`module.py:272,473,899`).
pub fn shape_archived_filter(shape: ReadShape) -> ArchivedFilter {
    match shape {
        ReadShape::ModuleList | ReadShape::ModuleDetail => ArchivedFilter::Live,
        ReadShape::ArchivedModuleList => ArchivedFilter::Archived,
        ReadShape::ModuleIssueQueryset
        | ReadShape::ModuleIssueListGet
        | ReadShape::ModuleIssueDetailGet
        | ReadShape::ModuleIssueDelete => ArchivedFilter::Any,
    }
}

/// Whether the shape joins `project_members` on the acting user, i.e.
/// the handler must supply it (`module.py:557-559,763-765`). Only the
/// M4/M5 querysets do — M1/M2/M3 list for every project member alike
/// (ported bug 2: the asymmetry vs cycle querysets).
pub fn shape_needs_member(shape: ReadShape) -> bool {
    matches!(shape, ReadShape::ModuleIssueQueryset)
}

/// Whether the shape selects `DISTINCT`: only the M4/M5 querysets
/// (`module.py:568,774`); M1/M2/M3 and both GET shapes do not.
pub fn shape_is_distinct(shape: ReadShape) -> bool {
    matches!(shape, ReadShape::ModuleIssueQueryset)
}

/// The order default a shape carries: descending `created_at` for every
/// queryset chain, ascending `created_at` for the M4/M5 GET chains.
pub fn shape_order_default(shape: ReadShape) -> &'static str {
    match shape {
        ReadShape::ModuleIssueListGet | ReadShape::ModuleIssueDetailGet => ISSUE_GET_ORDER_DEFAULT,
        _ => QUERYSET_ORDER_DEFAULT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_parse_mirrors_django() {
        // Kwarg chains: absent → "-created_at".
        assert_eq!(
            parse_order(None, QUERYSET_ORDER_DEFAULT),
            OrderBy::new("created_at", true)
        );
        // GET chains: absent → "created_at" ascending.
        assert_eq!(
            parse_order(None, ISSUE_GET_ORDER_DEFAULT),
            OrderBy::new("created_at", false)
        );
        // Leading '-' selects descending; anything else is ascending.
        assert_eq!(
            parse_order(Some("-name"), QUERYSET_ORDER_DEFAULT),
            OrderBy::new("name", true)
        );
        assert_eq!(
            parse_order(Some("name"), QUERYSET_ORDER_DEFAULT),
            OrderBy::new("name", false)
        );
        // Passthrough: unknown columns are the database's problem, like
        // Django's FieldError-at-evaluation.
        assert_eq!(
            parse_order(Some("no_such_col"), QUERYSET_ORDER_DEFAULT),
            OrderBy::new("no_such_col", false)
        );
    }

    #[test]
    fn shape_table() {
        // Archived filter per shape (module.py:272,473,899; issue paths: none).
        assert_eq!(
            shape_archived_filter(ReadShape::ModuleList),
            ArchivedFilter::Live
        );
        assert_eq!(
            shape_archived_filter(ReadShape::ModuleDetail),
            ArchivedFilter::Live
        );
        assert_eq!(
            shape_archived_filter(ReadShape::ArchivedModuleList),
            ArchivedFilter::Archived
        );
        for shape in [
            ReadShape::ModuleIssueQueryset,
            ReadShape::ModuleIssueListGet,
            ReadShape::ModuleIssueDetailGet,
            ReadShape::ModuleIssueDelete,
        ] {
            assert_eq!(shape_archived_filter(shape), ArchivedFilter::Any);
        }
        // Only the issue querysets need the acting user (ported bug 2).
        for shape in [
            ReadShape::ModuleList,
            ReadShape::ModuleDetail,
            ReadShape::ArchivedModuleList,
            ReadShape::ModuleIssueListGet,
            ReadShape::ModuleIssueDetailGet,
            ReadShape::ModuleIssueDelete,
        ] {
            assert!(!shape_needs_member(shape), "{shape:?}");
        }
        assert!(shape_needs_member(ReadShape::ModuleIssueQueryset));
        // DISTINCT only on the issue querysets.
        for shape in [
            ReadShape::ModuleList,
            ReadShape::ModuleDetail,
            ReadShape::ArchivedModuleList,
            ReadShape::ModuleIssueListGet,
            ReadShape::ModuleIssueDetailGet,
            ReadShape::ModuleIssueDelete,
        ] {
            assert!(!shape_is_distinct(shape), "{shape:?}");
        }
        assert!(shape_is_distinct(ReadShape::ModuleIssueQueryset));
        // Order defaults: descending everywhere except the GET shapes.
        for shape in [
            ReadShape::ModuleList,
            ReadShape::ModuleDetail,
            ReadShape::ArchivedModuleList,
            ReadShape::ModuleIssueQueryset,
            ReadShape::ModuleIssueDelete,
        ] {
            assert_eq!(shape_order_default(shape), "-created_at", "{shape:?}");
        }
        assert_eq!(
            shape_order_default(ReadShape::ModuleIssueListGet),
            "created_at"
        );
        assert_eq!(
            shape_order_default(ReadShape::ModuleIssueDetailGet),
            "created_at"
        );
    }

    #[test]
    fn issue_detail_get_is_unrouted() {
        // api/urls/module.py:33-37 wires delete only (ported bug 6).
        assert_eq!(MODULE_ISSUE_DETAIL_ROUTED_METHODS, &["delete"]);
    }
}
