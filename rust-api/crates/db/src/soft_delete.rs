//! Soft-delete read scope, write statements, and view DDL.
//!
//! Mirrors `SoftDeleteModel` / `SoftDeletionManager` /
//! `SoftDeletionQuerySet` (`pi_dash/db/mixins.py`):
//!
//! - Reads go through the `deleted_at IS NULL` scope: the default manager
//!   filters it, while `all_objects` (unscoped reads, e.g. admin restore)
//!   does not. The kernel exposes both: [`active_condition`] for query
//!   builders and [`active_view_ddl`] for the per-table read view the
//!   Porting guide mandates.
//! - `delete(soft=True)` stamps `deleted_at` (a queryset `delete()` does
//!   `update(deleted_at=now())`); `delete(soft=False)` removes the row.
//!   Both are [`build_soft_delete`] / [`build_hard_delete`].
//!
//! Partial unique indexes stay as they are (schema owner is Django until
//! stage 8), so this module emits no DDL besides the read views.

use sea_query::{
    Alias, Condition, DeleteStatement, Expr, Keyword, Query, SimpleExpr, UpdateStatement,
};

/// The soft-delete marker column, as on every `SoftDeleteModel`.
pub const DELETED_AT: &str = "deleted_at";

/// The default read scope: live rows only.
///
/// Apply to every read unless the caller explicitly needs deleted rows
/// (the `all_objects` escape hatch).
pub fn active_condition() -> Condition {
    Condition::all().add(Expr::col(Alias::new(DELETED_AT)).is_null())
}

/// `UPDATE <table> SET deleted_at = CURRENT_TIMESTAMP WHERE <pk> = $1`
/// (rendered form depends on the builder).
///
/// Mirrors the instance `delete(soft=True)`: stamp the marker, keep the
/// row. `CURRENT_TIMESTAMP` is the SQL spelling of `timezone.now()`.
pub fn build_soft_delete(table: &str, pk_col: &str, pk: sea_query::Value) -> UpdateStatement {
    let mut stmt = Query::update();
    stmt.table(Alias::new(table.to_owned()))
        .value(
            Alias::new(DELETED_AT.to_owned()),
            SimpleExpr::Keyword(Keyword::CurrentTimestamp),
        )
        .and_where(Expr::col(Alias::new(pk_col.to_owned())).eq(pk));
    stmt
}

/// `UPDATE <table> SET deleted_at = CURRENT_TIMESTAMP WHERE <cond>`,
/// the queryset-`delete()` form (`update(deleted_at=now())` over a scope).
pub fn build_soft_delete_where(table: &str, scope: Condition) -> UpdateStatement {
    let mut stmt = Query::update();
    stmt.table(Alias::new(table.to_owned()))
        .value(
            Alias::new(DELETED_AT.to_owned()),
            SimpleExpr::Keyword(Keyword::CurrentTimestamp),
        )
        .cond_where(scope);
    stmt
}

/// `DELETE FROM <table> WHERE <pk> = $1`: the `delete(soft=False)` path.
pub fn build_hard_delete(table: &str, pk_col: &str, pk: sea_query::Value) -> DeleteStatement {
    let mut stmt = Query::delete();
    stmt.from_table(Alias::new(table.to_owned()))
        .and_where(Expr::col(Alias::new(pk_col.to_owned())).eq(pk));
    stmt
}

/// `CREATE OR REPLACE VIEW <table>_active AS SELECT * FROM <table> WHERE
/// deleted_at IS NULL`: the per-table read view. Reads serve from the
/// view; writes hit the table, so partial unique indexes keep working.
pub fn active_view_ddl(table: &str) -> String {
    format!(
        "CREATE OR REPLACE VIEW {table}_active AS SELECT * FROM \"{table}\" WHERE \"{DELETED_AT}\" IS NULL;"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::PostgresQueryBuilder;

    fn id(value: &str) -> sea_query::Value {
        sea_query::Value::String(Some(value.to_owned().into()))
    }

    #[test]
    fn active_scope_filters_deleted_rows() {
        let mut select = Query::select();
        select
            .column(Alias::new("id"))
            .from(Alias::new("issue"))
            .cond_where(active_condition());
        let sql = select.to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "id" FROM "issue" WHERE "deleted_at" IS NULL"#
        );
    }

    #[test]
    fn soft_delete_stamps_marker_for_one_row() {
        let sql = build_soft_delete("issue", "id", id("i-1")).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"UPDATE "issue" SET "deleted_at" = CURRENT_TIMESTAMP WHERE "id" = 'i-1'"#
        );
    }

    #[test]
    fn soft_delete_where_scopes_the_stamp() {
        let scope = Condition::all().add(Expr::col(Alias::new("project_id")).eq("p-1"));
        let sql = build_soft_delete_where("issue", scope).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"UPDATE "issue" SET "deleted_at" = CURRENT_TIMESTAMP WHERE "project_id" = 'p-1'"#
        );
    }

    #[test]
    fn hard_delete_removes_the_row() {
        let sql = build_hard_delete("issue", "id", id("i-1")).to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"DELETE FROM "issue" WHERE "id" = 'i-1'"#);
    }

    #[test]
    fn view_ddl_names_table_active_view() {
        assert_eq!(
            active_view_ddl("issue"),
            r#"CREATE OR REPLACE VIEW issue_active AS SELECT * FROM "issue" WHERE "deleted_at" IS NULL;"#
        );
    }
}
