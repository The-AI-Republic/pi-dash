//! Run SeaORM-built statements on an sqlx executor, including an open
//! `sqlx::Transaction`.
//!
//! A SeaORM transaction cannot adopt an `sqlx::Transaction`, and sqlx cannot
//! join a SeaORM one (`.ai_design/rust_orm/design.md`). Much of rust-api runs
//! its reads and writes inside sqlx transactions, often with row locks, and
//! passes them to shared kernels. Rewriting every transaction boundary would
//! block the raw-SQL migration (`rust-api/docs/orm-migration.md`), so this
//! bridge lets that code build its statements with SeaORM entities and the
//! query builder, then run them on the transaction it already holds:
//!
//! ```ignore
//! let runner: Option<runner::Model> = orm_tx::fetch_optional(
//!     &mut *tx,
//!     &runner::Entity::find_by_id(id).lock_exclusive().into_query(),
//! )
//! .await?;
//! orm_tx::execute(
//!     &mut *tx,
//!     &runner::Entity::update_many()
//!         .col_expr(runner::Column::RevokedAt, Expr::value(now))
//!         .filter(runner::Column::Id.eq(id))
//!         .into_query(),
//! )
//! .await?;
//! ```
//!
//! Statements are bound through `sea-query-binder`, the binder SeaORM's own
//! sqlx driver uses, and rows decode through SeaORM's `FromQueryResult`
//! (entity `Model`s, partial models and `#[derive(FromQueryResult)]`
//! structs), so names and types come from the entities, never from
//! positions. The executor is any sqlx Postgres executor: `&mut *tx`,
//! `&mut *conn` or `&PgPool`.

use sea_orm::{DbErr, FromQueryResult, QueryResult};
use sea_query::PostgresQueryBuilder;
use sea_query_binder::SqlxBinder;
use sqlx::postgres::{PgQueryResult, Postgres};

/// Build `statement` for Postgres with its bound values.
fn build(statement: &impl SqlxBinder) -> (String, sea_query_binder::SqlxValues) {
    statement.build_sqlx(PostgresQueryBuilder)
}

fn decode<M: FromQueryResult>(row: sqlx::postgres::PgRow) -> Result<M, sqlx::Error> {
    M::from_query_result(&QueryResult::from(row), "").map_err(db_err_to_sqlx)
}

/// A SeaORM decode error as the sqlx error callers already handle.
pub fn db_err_to_sqlx(error: DbErr) -> sqlx::Error {
    match error {
        DbErr::Query(sea_orm::RuntimeErr::SqlxError(inner))
        | DbErr::Exec(sea_orm::RuntimeErr::SqlxError(inner))
        | DbErr::Conn(sea_orm::RuntimeErr::SqlxError(inner)) => inner,
        other => sqlx::Error::Decode(Box::new(other)),
    }
}

/// Every row of a SeaORM-built query, decoded into `M`.
pub async fn fetch_all<'e, M, X>(
    executor: X,
    statement: &impl SqlxBinder,
) -> Result<Vec<M>, sqlx::Error>
where
    M: FromQueryResult,
    X: sqlx::Executor<'e, Database = Postgres>,
{
    let (sql, values) = build(statement);
    // raw-sql: the SeaORM <-> sqlx-transaction bridge; the statement is SeaORM-built.
    let rows = sqlx::query_with(&sql, values).fetch_all(executor).await?;
    rows.into_iter().map(decode).collect()
}

/// At most one row of a SeaORM-built query, decoded into `M`. The statement
/// is run as built: add `.limit(1)` to it when only the first row matters.
pub async fn fetch_optional<'e, M, X>(
    executor: X,
    statement: &impl SqlxBinder,
) -> Result<Option<M>, sqlx::Error>
where
    M: FromQueryResult,
    X: sqlx::Executor<'e, Database = Postgres>,
{
    let (sql, values) = build(statement);
    // raw-sql: the SeaORM <-> sqlx-transaction bridge; the statement is SeaORM-built.
    let row = sqlx::query_with(&sql, values)
        .fetch_optional(executor)
        .await?;
    row.map(decode).transpose()
}

/// Exactly one row of a SeaORM-built query, decoded into `M`
/// (`sqlx::Error::RowNotFound` when there is none).
pub async fn fetch_one<'e, M, X>(executor: X, statement: &impl SqlxBinder) -> Result<M, sqlx::Error>
where
    M: FromQueryResult,
    X: sqlx::Executor<'e, Database = Postgres>,
{
    let (sql, values) = build(statement);
    // raw-sql: the SeaORM <-> sqlx-transaction bridge; the statement is SeaORM-built.
    let row = sqlx::query_with(&sql, values).fetch_one(executor).await?;
    decode(row)
}

/// Run a SeaORM-built INSERT / UPDATE / DELETE (or any statement whose rows
/// are not read) and return the affected-row count.
pub async fn execute<'e, X>(
    executor: X,
    statement: &impl SqlxBinder,
) -> Result<PgQueryResult, sqlx::Error>
where
    X: sqlx::Executor<'e, Database = Postgres>,
{
    let (sql, values) = build(statement);
    // raw-sql: the SeaORM <-> sqlx-transaction bridge; the statement is SeaORM-built.
    sqlx::query_with(&sql, values).execute(executor).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{agent_run, runner};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect, QueryTrait};
    use uuid::Uuid;

    #[test]
    fn builds_a_select_with_bound_values() {
        let id = Uuid::new_v4();
        let query = runner::Entity::find_by_id(id).lock_exclusive().into_query();
        let (sql, _values) = build(&query);
        assert!(sql.starts_with(r#"SELECT "runner"."id""#), "{sql}");
        assert!(sql.contains(r#"WHERE "runner"."id" = $1"#), "{sql}");
        assert!(sql.ends_with("FOR UPDATE"), "{sql}");
    }

    #[test]
    fn builds_an_update_with_bound_values() {
        let query = agent_run::Entity::update_many()
            .col_expr(
                agent_run::Column::Status,
                sea_query::Expr::value("cancelled"),
            )
            .filter(agent_run::Column::Id.eq(Uuid::nil()))
            .into_query();
        let (sql, _values) = build(&query);
        assert_eq!(
            sql,
            r#"UPDATE "agent_run" SET "status" = $1 WHERE "agent_run"."id" = $2"#
        );
    }

    /// Reads and writes through the bridge inside one sqlx transaction see
    /// each other, and decode into entity models by name.
    #[tokio::test]
    async fn live_round_trip_inside_an_sqlx_transaction() {
        let url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                return;
            }
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect");
        let mut tx = pool.begin().await.expect("begin");
        let workspace = Uuid::new_v4();
        let owner = Uuid::new_v4();
        // A session-local table shaped like the columns under test, so the
        // test needs no seeded rows: `pidash_db` has no seed helpers.
        sqlx::query(
            "CREATE TEMP TABLE runner (LIKE public.runner INCLUDING DEFAULTS) ON COMMIT DROP",
        )
        .execute(&mut *tx)
        .await
        .expect("temp table");
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO runner (id, created_at, updated_at, owner_id, workspace_id, pod_id, name, \
             host_label, provisioning, visibility, refresh_token_hash, refresh_token_fingerprint, \
             refresh_token_generation, previous_refresh_token_hash, \
             access_token_signing_key_version, enrollment_token_hash, \
             enrollment_token_fingerprint, capabilities, status, os, arch, runner_version, \
             dev_metadata, protocol_version, revoked_reason) VALUES ($1, now(), now(), $2, $3, $4, \
             'bridge', '', 'desktop_bundled', 0, '', '', 0, '', 1, '', '', '[]', 'online', '', '', \
             '', '{}', 1, '')",
        )
        .bind(id)
        .bind(owner)
        .bind(workspace)
        .bind(Uuid::new_v4())
        .execute(&mut *tx)
        .await
        .expect("seed");
        let done = execute(
            &mut *tx,
            &runner::Entity::update_many()
                .col_expr(runner::Column::Status, sea_query::Expr::value("revoked"))
                .filter(runner::Column::Id.eq(id))
                .into_query(),
        )
        .await
        .expect("update");
        assert_eq!(done.rows_affected(), 1);
        let found: Option<runner::Model> =
            fetch_optional(&mut *tx, &runner::Entity::find_by_id(id).into_query())
                .await
                .expect("read");
        let found = found.expect("row");
        assert_eq!(
            (found.id, found.status.as_str(), found.name.as_str()),
            (id, "revoked", "bridge")
        );
        #[derive(Debug, PartialEq, sea_orm::FromQueryResult)]
        struct NameRow {
            name: String,
        }
        let names: Vec<NameRow> = fetch_all(
            &mut *tx,
            &runner::Entity::find()
                .select_only()
                .column(runner::Column::Name)
                .filter(runner::Column::WorkspaceId.eq(workspace))
                .into_query(),
        )
        .await
        .expect("names");
        assert_eq!(
            names,
            vec![NameRow {
                name: "bridge".to_owned()
            }]
        );
        tx.rollback().await.expect("rollback");
    }
}
