//! Sticky list querysets (PIDASHCONV-409, D-21 stage 5).
//!
//! Ports query units 4-5 of `apps/api/pi_dash/api/views/sticky.py` to
//! sea-query builders (drift baseline `01a93e17`):
//!
//! * Unit 4 — `StickyViewSet.get_queryset` (`sticky.py:30-37`):
//!   `super().get_queryset()` (plain `.all()`, `views/base.py:236-241`)
//!   `.filter(workspace__slug=slug).filter(owner_id=user.id).distinct()`.
//! * Unit 5 — `list` ordering + search (`sticky.py:66-70`):
//!   `.order_by("-created_at")`, and when `query` is truthy,
//!   `.filter(description_stripped__icontains=query)`; pagination
//!   (`default_per_page=20`) is handler-owned.
//!
//! Fixture oracle: `fixtures/v1_assets/fx-q-sticky.json`. Both shapes
//! were checked against live Django 4.2 `str(queryset.query)` output;
//! the unit tests pin the fragments so transcription drift fails the
//! build. Builder contract, read-vs-view rule, and the `.get()`
//! LIMIT note are documented on [`super::asset_queries`] and apply
//! here unchanged (this module has no `.get()` paths).
//!
//! # The `icontains` port (Semantic traps)
//!
//! Django 4.2 on Postgres renders
//! `description_stripped__icontains=q` as
//! `UPPER("stickies"."description_stripped"::text) LIKE UPPER($3)`
//! with the pattern wrapped in Rust-side `%…%`
//! ([`icontains_pattern`]). Three traps, all ported exactly:
//!
//! * Case folding is `UPPER()` on **both** sides — not `LOWER()`, and
//!   not Rust-side folding. (The foundation `filter.rs` kernel folds
//!   with `LOWER()`; same rows, different text — this module follows
//!   the fixture's Django text, and `filter.rs` is read-only anyway.)
//! * The `::text` cast stays: it is part of Django's emitted SQL for
//!   this lookup.
//! * `prep_for_like_query` escaping happens in **Python** before the
//!   parameter is sent: `\` → `\\`, `%` → `\%`, `_` → `\_`, then the
//!   whole thing is wrapped in `%`. There is no `ESCAPE` clause —
//!   Postgres `LIKE` treats backslash as the default escape.
//!   [`icontains_pattern`] mirrors that byte for byte (verified live:
//!   `hello%_x` → `%hello\%\_x%`).
//!
//! `NULL` `description_stripped` rows never match (`UPPER(NULL) LIKE
//! …` is `NULL`, filtered by `WHERE`) — no extra predicate is added.
//! The falsy guard mirrors the view: `query` defaults to `False` and
//! only a truthy value adds the predicate (`sticky.py:66-69`), so
//! `None` and `""` both yield the unfiltered list.
//!
//! Out of scope (sibling D-21 issues): `StickySerializer` shaping
//! (PIDASHCONV-392/401), retrieve/update/destroy via `get_object`
//! (handlers, PIDASHCONV-419/421/423/426), and the workspace-membership
//! permission gate (PIDASHCONV-415).

use sea_query::{Alias, Condition, Expr, JoinType, Order, Query};

use super::model::sticky;

/// `workspaces` table (no db-layer port owns it yet; literal matches
/// the Django table name, same as `queries_stateest::WORKSPACE_TABLE`).
const WORKSPACE_TABLE: &str = "workspaces";

/// Project `columns` of `table` (same helper shape as
/// [`super::asset_queries`] and the merged precedents).
fn select_table_columns(sel: &mut sea_query::SelectStatement, table: &str, columns: &[&str]) {
    for col in columns {
        sel.column((Alias::new(table.to_owned()), Alias::new((*col).to_owned())));
    }
}

/// `INNER JOIN "workspaces" ON ("stickies"."workspace_id" =
/// "workspaces"."id")` (the `workspace__slug` traversal).
fn join_workspace(sel: &mut sea_query::SelectStatement) {
    sel.join(
        JoinType::InnerJoin,
        Alias::new(WORKSPACE_TABLE.to_owned()),
        Condition::all().add(
            Expr::col((
                Alias::new(sticky::TABLE.to_owned()),
                Alias::new("workspace_id"),
            ))
            .equals((Alias::new(WORKSPACE_TABLE), Alias::new("id"))),
        ),
    );
}

/// `get_queryset` scope shared by every `StickyViewSet` action
/// (`sticky.py:30-37`): soft-delete-manager conjunct, workspace slug,
/// owner id, `SELECT DISTINCT`. Predicate order follows the chained
/// `.filter(workspace__slug).filter(owner_id)` calls as emitted by
/// live Django (verified: slug before owner).
///
/// Binds: `$1` workspace slug, `$2` owner (acting-user) id.
fn sticky_scope() -> sea_query::SelectStatement {
    let mut sel = Query::select();
    sel.distinct();
    select_table_columns(&mut sel, sticky::TABLE, sticky::COLUMNS);
    sel.from(Alias::new(sticky::TABLE.to_owned()));
    join_workspace(&mut sel);
    sel.cond_where(
        Condition::all()
            .add(Expr::col((Alias::new(sticky::TABLE), Alias::new("deleted_at"))).is_null())
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("slug"))).eq(Expr::cust("$1")))
            .add(
                Expr::col((Alias::new(sticky::TABLE), Alias::new("owner_id"))).eq(Expr::cust("$2")),
            ),
    );
    sel
}

/// The `icontains` predicate exactly as Django 4.2 renders it (see the
/// module docs): `UPPER("stickies"."description_stripped"::text) LIKE
/// UPPER($3)`. A fixed fragment — the table, column, cast, and both
/// `UPPER()` calls are Django's fixed text, so `Expr::cust` carries it
/// whole (same technique the merged `module_queries` precedent uses
/// for fixed fragments).
fn icontains_predicate() -> sea_query::SimpleExpr {
    Expr::cust(format!(
        "UPPER(\"{}\".\"description_stripped\"::text) LIKE UPPER($3)",
        sticky::TABLE
    ))
}

/// `list` queryset (`sticky.py:66-70`): the [`sticky_scope`] ordered
/// newest-first, plus the `icontains` predicate when `query` is
/// truthy. `None` and `""` both fall through to the unfiltered list
/// (the view's falsy guard); pagination stays handler-owned.
///
/// Binds: `$1` workspace slug, `$2` owner id, `$3` the
/// [`icontains_pattern`] of `query` (present only when the `LIKE`
/// predicate is).
pub fn sticky_list_sql(query: Option<&str>) -> String {
    use sea_query::PostgresQueryBuilder;
    let mut sel = sticky_scope();
    if query.is_some_and(|q| !q.is_empty()) {
        sel.cond_where(Condition::all().add(icontains_predicate()));
    }
    sel.order_by(
        (
            Alias::new(sticky::TABLE.to_owned()),
            Alias::new("created_at"),
        ),
        Order::Desc,
    );
    sel.to_string(PostgresQueryBuilder)
}

/// Django `prep_for_like_query` + `%…%` wrap for `icontains`
/// (`django/db/models/lookups.py`, `PatternLookup.process_rhs`):
/// backslash-escape `\`, `%`, `_` (in that order — the backslash pass
/// first so it is not re-escaped), then wrap the result in literal
/// `%` signs. Operates per character; the three metacharacters are
/// ASCII, so code-point iteration matches Python exactly.
///
/// Verified against live Django: `"hello%_x"` → `"%hello\\%\\_x%"`.
/// Handlers bind the return value as `$3`.
pub fn icontains_pattern(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 2);
    out.push('%');
    for ch in raw.chars() {
        if ch == '\\' || ch == '%' || ch == '_' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('%');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_base_matches_get_queryset_shape() {
        let sql = sticky_list_sql(None);
        // SELECT DISTINCT + manager scope + slug + owner (fx-q-sticky
        // get_queryset), newest-first.
        assert!(sql.starts_with("SELECT DISTINCT "), "{sql}");
        assert!(
            sql.contains("FROM \"stickies\" INNER JOIN \"workspaces\" ON \"stickies\".\"workspace_id\" = \"workspaces\".\"id\""),
            "{sql}"
        );
        assert!(sql.contains("\"stickies\".\"deleted_at\" IS NULL"), "{sql}");
        assert!(sql.contains("\"workspaces\".\"slug\" = ($1)"), "{sql}");
        assert!(sql.contains("\"stickies\".\"owner_id\" = ($2)"), "{sql}");
        assert!(
            sql.contains("ORDER BY \"stickies\".\"created_at\" DESC"),
            "{sql}"
        );
        // No search predicate without a truthy query.
        assert!(!sql.contains("LIKE"), "{sql}");
    }

    #[test]
    fn empty_query_is_unfiltered_like_absent() {
        assert_eq!(sticky_list_sql(Some("")), sticky_list_sql(None));
    }

    #[test]
    fn list_with_query_adds_django_icontains_text() {
        let sql = sticky_list_sql(Some("hello"));
        // Exact Django 4.2 rendering: UPPER() both sides, ::text cast.
        assert!(
            sql.contains("UPPER(\"stickies\".\"description_stripped\"::text) LIKE UPPER($3)"),
            "{sql}"
        );
        assert!(
            sql.contains("ORDER BY \"stickies\".\"created_at\" DESC"),
            "{sql}"
        );
        // DISTINCT survives the extra predicate.
        assert!(sql.starts_with("SELECT DISTINCT "), "{sql}");
    }

    #[test]
    fn icontains_pattern_escapes_like_django() {
        // Live-verified golden: backslash-escapes %, _, \ then wraps.
        assert_eq!(icontains_pattern("hello"), "%hello%");
        assert_eq!(icontains_pattern("hello%_x"), "%hello\\%\\_x%");
        assert_eq!(icontains_pattern("a\\b"), "%a\\\\b%");
        assert_eq!(icontains_pattern(""), "%%");
        // A query that is only wildcards still filters (it matches
        // almost everything, but the predicate is applied — the view
        // guards on truthiness, not on wildcard content).
        assert_eq!(icontains_pattern("%"), "%\\%%");
    }

    #[test]
    fn sticky_projection_covers_search_column() {
        let sql = sticky_list_sql(None);
        assert!(
            sql.contains("\"stickies\".\"description_stripped\""),
            "{sql}"
        );
        assert_eq!(sticky::COLUMNS.len(), 17, "sticky column count drift");
    }
}
