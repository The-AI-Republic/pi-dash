//! SeaORM on an open sqlx transaction.
//!
//! A SeaORM transaction cannot adopt an `sqlx::Transaction`, and sqlx cannot
//! join a SeaORM one (`.ai_design/rust_orm/design.md`). Much of rust-api runs
//! its reads and writes inside sqlx transactions, often with row locks, and
//! passes them to shared kernels. Rewriting every transaction boundary would
//! block the raw-SQL migration (`rust-api/docs/orm-migration.md`), so
//! [`TxConn`] wraps the connection that transaction holds as a SeaORM
//! [`ConnectionTrait`]: the whole SeaORM API runs on it unchanged.
//!
//! ```ignore
//! let conn = orm_tx::TxConn::new(&mut tx);
//! let runner = runner::Entity::find_by_id(id).lock_exclusive().one(&conn).await?;
//! runner::Entity::update_many()
//!     .col_expr(runner::Column::RevokedAt, Expr::value(now))
//!     .filter(runner::Column::Id.eq(id))
//!     .exec(&conn)
//!     .await?;
//! comment.insert(&conn).await?; // ActiveModelBehavior hooks run, RETURNING too
//! ```
//!
//! Statements bind through `sea-query-binder`, exactly as SeaORM's own
//! Postgres driver binds them, and errors are the `DbErr`s that driver
//! returns. `TxConn` borrows the connection; drop it (end its scope) to use
//! the transaction directly again. It deliberately does not implement
//! `TransactionTrait`: the transaction belongs to its sqlx owner, which
//! commits or rolls it back.

use sea_orm::{ConnectionTrait, DbBackend, DbErr, ExecResult, QueryResult, RuntimeErr, Statement};
use sea_query::Values;
use sea_query_binder::SqlxValues;
use sqlx::PgConnection;
use tokio::sync::Mutex;

/// A SeaORM connection over a borrowed sqlx Postgres connection, usually the
/// one an `sqlx::Transaction` holds (`TxConn::new(&mut tx)`).
pub struct TxConn<'c> {
    conn: Mutex<&'c mut PgConnection>,
}

impl<'c> TxConn<'c> {
    pub fn new(conn: &'c mut PgConnection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }
}

impl std::fmt::Debug for TxConn<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TxConn")
    }
}

fn query(stmt: &Statement) -> sqlx::query::Query<'_, sqlx::Postgres, SqlxValues> {
    let values = stmt.values.clone().unwrap_or(Values(Vec::new()));
    // raw-sql: the SeaORM <-> sqlx-transaction bridge; every statement is SeaORM-built.
    sqlx::query_with(&stmt.sql, SqlxValues(values))
}

fn exec_err(error: sqlx::Error) -> DbErr {
    DbErr::Exec(RuntimeErr::SqlxError(error))
}

fn query_err(error: sqlx::Error) -> DbErr {
    DbErr::Query(RuntimeErr::SqlxError(error))
}

#[async_trait::async_trait]
impl ConnectionTrait for TxConn<'_> {
    fn get_database_backend(&self) -> DbBackend {
        DbBackend::Postgres
    }

    async fn execute(&self, stmt: Statement) -> Result<ExecResult, DbErr> {
        let mut conn = self.conn.lock().await;
        query(&stmt)
            .execute(&mut **conn)
            .await
            .map(ExecResult::from)
            .map_err(exec_err)
    }

    async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, DbErr> {
        let mut conn = self.conn.lock().await;
        // raw-sql: `ConnectionTrait::execute_unprepared` (SeaORM's own escape hatch); a bare `&str` runs unprepared, like `raw_sql`.
        sqlx::Executor::execute(&mut **conn, sql)
            .await
            .map(ExecResult::from)
            .map_err(exec_err)
    }

    async fn query_one(&self, stmt: Statement) -> Result<Option<QueryResult>, DbErr> {
        let mut conn = self.conn.lock().await;
        query(&stmt)
            .fetch_optional(&mut **conn)
            .await
            .map(|row| row.map(QueryResult::from))
            .map_err(query_err)
    }

    async fn query_all(&self, stmt: Statement) -> Result<Vec<QueryResult>, DbErr> {
        let mut conn = self.conn.lock().await;
        query(&stmt)
            .fetch_all(&mut **conn)
            .await
            .map(|rows| rows.into_iter().map(QueryResult::from).collect())
            .map_err(query_err)
    }
}

/// Django's `IntegrityError` for a SeaORM failure: the wrapped sqlx error
/// is a SQLSTATE class 23 violation (unique, foreign key, not null, check),
/// as [`pidash_compat::db::is_integrity_error`] decides for sqlx errors.
pub fn is_integrity_violation(error: &DbErr) -> bool {
    match error {
        DbErr::Exec(RuntimeErr::SqlxError(inner)) | DbErr::Query(RuntimeErr::SqlxError(inner)) => {
            pidash_compat::db::is_integrity_error(inner)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{issue_comments, runner};
    use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QuerySelect, Set};
    use uuid::Uuid;

    /// `DATABASE_URL` pointing at a migrated database, else skip.
    async fn pool() -> Option<sqlx::PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return None;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect");
        let migrated: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('public.issue_comments')::text")
                .fetch_one(&pool)
                .await
                .expect("probe");
        if migrated.is_none() {
            eprintln!("skipping live-db test: the database is not migrated");
            return None;
        }
        Some(pool)
    }

    /// An entity insert (hooks included), a locked read, a bulk update and a
    /// partial read all run on one sqlx transaction, and each sees the
    /// previous statement's uncommitted write.
    #[tokio::test]
    async fn live_seaorm_on_an_sqlx_transaction() {
        let Some(pool) = pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin");
        // Session-local copies without foreign keys, so the test needs no
        // seeded parents; they shadow the real tables for this transaction.
        sqlx::query(
            "CREATE TEMP TABLE issue_comments (LIKE public.issue_comments INCLUDING DEFAULTS) \
             ON COMMIT DROP",
        )
        .execute(&mut *tx)
        .await
        .expect("temp table");
        let issue_id = Uuid::new_v4();
        {
            let conn = TxConn::new(&mut tx);
            let comment = issue_comments::ActiveModel {
                id: Set(Uuid::new_v4()),
                created_at: Set(chrono::Utc::now().into()),
                updated_at: Set(chrono::Utc::now().into()),
                comment_html: Set("<p>Hello <b>bridge</b></p>".to_owned()),
                comment_json: Set(serde_json::json!({})),
                comment_stripped: Set(String::new()),
                access: Set("INTERNAL".to_owned()),
                issue_id: Set(issue_id),
                project_id: Set(Uuid::new_v4()),
                workspace_id: Set(Uuid::new_v4()),
                attachments: Set(Vec::new()),
                labels: Set(Vec::new()),
                speaker_type: Set("human".to_owned()),
                speaker_label: Set(String::new()),
                ..Default::default()
            };
            let inserted = comment.insert(&conn).await.expect("insert");
            // `before_save` (comment_hooks) recomputed the stripped text.
            assert_eq!(inserted.comment_stripped, "Hello bridge");

            let locked = issue_comments::Entity::find_by_id(inserted.id)
                .lock_exclusive()
                .one(&conn)
                .await
                .expect("locked read")
                .expect("row");
            assert_eq!(locked.id, inserted.id);

            let done = issue_comments::Entity::update_many()
                .col_expr(
                    issue_comments::Column::Access,
                    sea_query::Expr::value("EXTERNAL"),
                )
                .filter(issue_comments::Column::IssueId.eq(issue_id))
                .exec(&conn)
                .await
                .expect("update");
            assert_eq!(done.rows_affected, 1);

            #[derive(Debug, PartialEq, sea_orm::FromQueryResult)]
            struct AccessRow {
                access: String,
            }
            let rows = issue_comments::Entity::find()
                .select_only()
                .column(issue_comments::Column::Access)
                .filter(issue_comments::Column::IssueId.eq(issue_id))
                .into_model::<AccessRow>()
                .all(&conn)
                .await
                .expect("partial read");
            assert_eq!(
                rows,
                vec![AccessRow {
                    access: "EXTERNAL".to_owned()
                }]
            );
            assert!(runner::Entity::find_by_id(Uuid::new_v4())
                .one(&conn)
                .await
                .expect("miss")
                .is_none());
        }
        // The borrow ended: the transaction is usable directly again.
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM issue_comments")
            .fetch_one(&mut *tx)
            .await
            .expect("count");
        assert_eq!(n, 1);
        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn live_errors_are_the_drivers_db_errs() {
        let Some(pool) = pool().await else {
            return;
        };
        let mut tx = pool.begin().await.expect("begin");
        let conn = TxConn::new(&mut tx);
        let error = conn
            .query_one(Statement::from_string(
                DbBackend::Postgres,
                "SELECT * FROM no_such_table",
            ))
            .await
            .expect_err("missing table");
        assert!(
            matches!(
                error,
                DbErr::Query(RuntimeErr::SqlxError(sqlx::Error::Database(_)))
            ),
            "{error:?}"
        );
    }
}
