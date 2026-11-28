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
//! build. Statement contract, read-vs-view rule, and the `.get()`
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
//! * The text cast stays: it is part of Django's emitted SQL for this
//!   lookup. (sea-query renders it as `CAST(… AS text)`, which is
//!   semantically identical to Django's `::text` spelling.)
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

/// `get_queryset` FROM/JOIN/WHERE shared by the list and count
/// statements (`sticky.py:30-37`): the `workspace__slug` traversal plus
/// the soft-delete-manager conjunct and the owner scope. Predicate
/// order follows the chained `.filter(workspace__slug).filter(owner_id)`
/// calls as emitted by live Django (verified: slug before owner).
///
/// Binds: `$1` workspace slug, `$2` owner (acting-user) id — in the
/// position the caller binds them.
fn sticky_base(slug: &str, owner_id: uuid::Uuid) -> sea_query::SelectStatement {
    let mut sel = Query::select();
    sel.from(Alias::new(sticky::TABLE.to_owned()));
    join_workspace(&mut sel);
    sel.cond_where(
        Condition::all()
            .add(Expr::col((Alias::new(sticky::TABLE), Alias::new("deleted_at"))).is_null())
            .add(Expr::col((Alias::new(WORKSPACE_TABLE), Alias::new("slug"))).eq(slug))
            .add(Expr::col((Alias::new(sticky::TABLE), Alias::new("owner_id"))).eq(owner_id)),
    );
    sel
}

/// The `icontains` predicate as Django 4.2 renders it (see the module
/// docs): `UPPER("stickies"."description_stripped"::text) LIKE
/// UPPER($3)` — both `UPPER()`s, the text cast, and the bound
/// pattern. `None` column values never match (`UPPER(NULL)` is `NULL`).
fn icontains_predicate(pattern: &str) -> sea_query::SimpleExpr {
    let text_col: sea_query::SimpleExpr = sea_query::Func::upper(
        Expr::col((
            Alias::new(sticky::TABLE.to_owned()),
            Alias::new("description_stripped"),
        ))
        .cast_as(Alias::new("text")),
    )
    .into();
    let upper_pattern: sea_query::SimpleExpr = sea_query::Func::upper(Expr::value(pattern)).into();
    text_col.binary(sea_query::BinOper::Like, upper_pattern)
}

/// `list` queryset (`sticky.py:66-70`): the [`sticky_base`] scope with
/// `SELECT DISTINCT`, ordered newest-first, plus the `icontains`
/// predicate when `query` is truthy. `None` and `""` both fall through
/// to the unfiltered list (the view's falsy guard); pagination stays
/// handler-owned (`LIMIT`/`OFFSET` on the returned statement).
///
/// Binds: `$1` workspace slug, `$2` owner id, `$3` the
/// [`icontains_pattern`] of `query` (present only when the `LIKE`
/// predicate is).
pub fn sticky_list(
    slug: &str,
    owner_id: uuid::Uuid,
    query: Option<&str>,
) -> sea_query::SelectStatement {
    let mut sel = sticky_base(slug, owner_id);
    sel.distinct();
    select_table_columns(&mut sel, sticky::TABLE, sticky::COLUMNS);
    if query.is_some_and(|q| !q.is_empty()) {
        sel.cond_where(Condition::all().add(icontains_predicate(&icontains_pattern(
            query.expect("truthy"),
        ))));
    }
    sel.order_by(
        (
            Alias::new(sticky::TABLE.to_owned()),
            Alias::new("created_at"),
        ),
        Order::Desc,
    );
    sel
}

/// `queryset.count()` over the same scope (`sticky.py` list envelope):
/// `COUNT(DISTINCT "stickies"."id")` over the filtered rows, no
/// ordering (equivalent to the DISTINCT-list queryset's count — ids are
/// unique; the join is many-to-one, so no row can double-count).
///
/// Binds: `$1` workspace slug, `$2` owner id, `$3` the
/// [`icontains_pattern`] of `query` (present only when the `LIKE`
/// predicate is). The count column is named `n`.
pub fn sticky_list_count(
    slug: &str,
    owner_id: uuid::Uuid,
    query: Option<&str>,
) -> sea_query::SelectStatement {
    let mut sel = sticky_base(slug, owner_id);
    sel.expr_as(
        Expr::col((Alias::new(sticky::TABLE), Alias::new("id"))).count_distinct(),
        Alias::new("n"),
    );
    if query.is_some_and(|q| !q.is_empty()) {
        sel.cond_where(Condition::all().add(icontains_predicate(&icontains_pattern(
            query.expect("truthy"),
        ))));
    }
    sel
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
    use sea_query::PostgresQueryBuilder;

    fn test_slug() -> &'static str {
        "acme"
    }

    fn test_owner() -> uuid::Uuid {
        uuid::Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap()
    }

    #[test]
    fn list_base_matches_get_queryset_shape() {
        let sql = sticky_list(test_slug(), test_owner(), None).to_string(PostgresQueryBuilder);
        // SELECT DISTINCT + manager scope + slug + owner (fx-q-sticky
        // get_queryset), newest-first.
        assert!(sql.starts_with("SELECT DISTINCT "), "{sql}");
        assert!(
            sql.contains("FROM \"stickies\" INNER JOIN \"workspaces\" ON \"stickies\".\"workspace_id\" = \"workspaces\".\"id\""),
            "{sql}"
        );
        assert!(sql.contains("\"stickies\".\"deleted_at\" IS NULL"), "{sql}");
        assert!(sql.contains("\"workspaces\".\"slug\" = 'acme'"), "{sql}");
        assert!(
            sql.contains("\"stickies\".\"owner_id\" = '33333333-3333-3333-3333-333333333333'"),
            "{sql}"
        );
        assert!(
            sql.contains("ORDER BY \"stickies\".\"created_at\" DESC"),
            "{sql}"
        );
        // No search predicate without a truthy query.
        assert!(!sql.contains("LIKE"), "{sql}");
    }

    #[test]
    fn empty_query_is_unfiltered_like_absent() {
        assert_eq!(
            sticky_list(test_slug(), test_owner(), Some("")).to_string(PostgresQueryBuilder),
            sticky_list(test_slug(), test_owner(), None).to_string(PostgresQueryBuilder)
        );
    }

    #[test]
    fn list_with_query_adds_django_icontains_text() {
        let sql =
            sticky_list(test_slug(), test_owner(), Some("hello")).to_string(PostgresQueryBuilder);
        // Django 4.2 rendering: UPPER() both sides, text cast (sea-query
        // spells the cast as CAST(… AS text); PG semantics identical).
        assert!(
            sql.contains(
                "UPPER(CAST(\"stickies\".\"description_stripped\" AS text)) LIKE UPPER('%hello%')"
            ),
            "{sql}"
        );
        assert!(
            sql.contains("ORDER BY \"stickies\".\"created_at\" DESC"),
            "{sql}"
        );
        // DISTINCT survives the extra predicate.
        assert!(sql.starts_with("SELECT DISTINCT "), "{sql}");
        // The count carries the same predicate without ordering.
        let count = sticky_list_count(test_slug(), test_owner(), Some("hello"))
            .to_string(PostgresQueryBuilder);
        assert!(
            count.contains("COUNT(DISTINCT \"stickies\".\"id\") AS \"n\""),
            "{count}"
        );
        assert!(
            count.contains(
                "UPPER(CAST(\"stickies\".\"description_stripped\" AS text)) LIKE UPPER('%hello%')"
            ),
            "{count}"
        );
        assert!(!count.contains("ORDER BY"), "{count}");
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
        let sql = sticky_list(test_slug(), test_owner(), None).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains("\"stickies\".\"description_stripped\""),
            "{sql}"
        );
        assert_eq!(sticky::COLUMNS.len(), 17, "sticky column count drift");
        // The count projects a single aggregate, no projection leak.
        let count =
            sticky_list_count(test_slug(), test_owner(), None).to_string(PostgresQueryBuilder);
        assert!(count.starts_with("SELECT COUNT(DISTINCT "), "{count}");
        assert!(
            !count.contains("\"stickies\".\"description_stripped\""),
            "{count}"
        );
    }

    async fn live_pool() -> Option<sqlx::PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return None;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        let migrated: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('public.stickies')::text")
                .fetch_one(&pool)
                .await
                .expect("probe");
        if migrated.is_none() {
            eprintln!("skipping live-db test: the database is not migrated");
            return None;
        }
        // Session-local copies without keys or constraints, so the test
        // needs no seeded parents; they shadow the real tables on this
        // single session. No ON COMMIT DROP: there is no explicit
        // transaction, so the tables would drop at the first commit.
        for table in ["stickies", "workspaces"] {
            let sql =
                format!("CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA");
            sqlx::query(&sql).execute(&pool).await.expect("temp table");
        }
        Some(pool)
    }

    /// The list + count statements against the shadow tables: filtering,
    /// case-insensitive search, ordering and the count agree. Skips
    /// without `DATABASE_URL`.
    #[tokio::test]
    async fn live_list_and_count() {
        use sea_orm::{ConnectionTrait, DbBackend, FromQueryResult};
        let Some(pool) = live_pool().await else {
            return;
        };
        let db = crate::pool::sea_orm_connection(&pool);
        let ws_id = uuid::Uuid::new_v4();
        let owner_id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now();
        sqlx::query("INSERT INTO workspaces (id, slug) VALUES ($1, 'acme')")
            .bind(ws_id)
            .execute(&pool)
            .await
            .expect("seed workspace");
        sqlx::query(
            "INSERT INTO stickies (id, created_at, updated_at, name, description,
                 description_html, description_stripped, logo_props,
                 workspace_id, owner_id, sort_order)
             VALUES ($1, $2, $2, 'first', '{}', '<p>x</p>', 'Hello World', '{}', $3, $4, 1.0)",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(now)
        .bind(ws_id)
        .bind(owner_id)
        .execute(&pool)
        .await
        .expect("seed sticky 1");
        sqlx::query(
            "INSERT INTO stickies (id, created_at, updated_at, name, description,
                 description_html, description_stripped, logo_props,
                 workspace_id, owner_id, sort_order)
             VALUES ($1, $2, $2, 'second', '{}', '<p>y</p>', 'goodbye', '{}', $3, $4, 2.0)",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(now)
        .bind(ws_id)
        .bind(owner_id)
        .execute(&pool)
        .await
        .expect("seed sticky 2");

        #[derive(FromQueryResult)]
        struct CountRow {
            n: i64,
        }

        let list = sticky_list("acme", owner_id, None);
        let rows = db
            .query_all(DbBackend::Postgres.build(&list))
            .await
            .expect("list");
        assert_eq!(rows.len(), 2);
        let count_stmt = sticky_list_count("acme", owner_id, None);
        let count_row = db
            .query_one(DbBackend::Postgres.build(&count_stmt))
            .await
            .expect("count")
            .expect("one row");
        let count_row = CountRow::from_query_result(&count_row, "").expect("decode");
        assert_eq!(count_row.n, 2);

        // Case-insensitive search narrows both.
        let filtered = sticky_list("acme", owner_id, Some("HELLO"));
        let rows = db
            .query_all(DbBackend::Postgres.build(&filtered))
            .await
            .expect("filtered");
        assert_eq!(rows.len(), 1);
        let count_stmt = sticky_list_count("acme", owner_id, Some("HELLO"));
        let count_row = db
            .query_one(DbBackend::Postgres.build(&count_stmt))
            .await
            .expect("count")
            .expect("one row");
        let count_row = CountRow::from_query_result(&count_row, "").expect("decode");
        assert_eq!(count_row.n, 1);

        // Wrong slug / owner miss both.
        assert!(db
            .query_all(DbBackend::Postgres.build(&sticky_list("other", owner_id, None)))
            .await
            .expect("list")
            .is_empty());
        assert!(db
            .query_all(DbBackend::Postgres.build(&sticky_list("acme", uuid::Uuid::new_v4(), None)))
            .await
            .expect("list")
            .is_empty());
    }
}
