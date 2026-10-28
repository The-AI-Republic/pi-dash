//! OSS-owned domain write services (stage 8 ORM adoption, design D3).
//!
//! Django gave every caller the write side effects for free: `Issue.save()`
//! assigns the sequence id, `post_save` receivers seed schedulers and fire
//! the orchestration transition, serializers queue activity and webhook
//! jobs. The Rust port kept each write inside its handler, so every new
//! caller copied the SQL and the side effects (design §1). This module tree
//! is the one public surface for writes to the covered tables (design §4):
//!
//! ```rust,no_run
//! use pidash_db::sea_orm_connection;
//! use pidash_services::domain::{self, WriteCtx};
//! # async fn demo(pool: &sqlx::PgPool, ctx: &WriteCtx) -> Result<(), domain::DomainError> {
//! let db = sea_orm_connection(pool);
//! domain::run(ctx, &db, |txn| {
//!     Box::pin(async move {
//!         // `txn` is the SeaORM transaction plus the after-commit queue;
//!         // entity calls, sea-query builders and raw SQL all run on it.
//!         # let _ = txn;
//!         Ok::<(), domain::DomainError>(())
//!     })
//! })
//! .await?;
//! # Ok(())
//! # }
//! ```
//!
//! One pool (D5): the [`sea_orm::DatabaseConnection`] wraps the existing
//! primary sqlx pool and opens no connections of its own. A SeaORM
//! transaction cannot adopt an `sqlx::Transaction` (`InnerConnection` is
//! `pub(crate)` in 1.1.20), so every statement inside a domain write runs
//! on the SeaORM transaction — entity calls directly, existing sea-query
//! builders through [`txn::WriteTxn::exec_builder`], the remaining raw SQL
//! (advisory locks, `SELECT … FOR UPDATE`) through
//! [`txn::WriteTxn::exec_raw`]. Domain writes never mix in sqlx
//! transactions. Reads stay on sqlx.
//!
//! After-commit queue: [`txn::CommitQueue`] keeps the
//! [`pidash_db::tx::AfterCommit`] semantics (steps run in order,
//! exactly once, after a successful commit; errors roll back and drop the
//! queue) with one difference — steps are async, because they call async
//! transition and enqueue functions. Today's handlers await the post-save
//! orchestration before responding, so the drain is awaited too: fire-and-
//! forget spawning here would change observable timing. The queue's doc
//! records the Django `on_commit` correspondence (failure modes, order,
//! outermost-only scope) with file citations.
//!
//! Effects that need services/jobs code but are triggered next to an entity
//! (scheduler seeding on workspace create) travel through the
//! [`txn::HookRegistry`] on [`txn::WriteTxn`]: the hook traits live here,
//! the implementations live wherever their dependencies live, and
//! `pidash-db` never depends on this crate.

pub mod activity;
pub mod assistant;
pub mod ctx;
pub mod error;
pub mod identity;
pub mod projects;
pub mod tokens;
pub mod transition;
pub mod txn;
pub mod workspaces;

pub use ctx::{WriteActor, WriteCtx, WriteOptions, WriteScope};
pub use error::DomainError;
pub use txn::{CommitQueue, HookRegistry, TxnConn, WriteTxn};

/// Run one domain write: begin, run `work` on the [`WriteTxn`], commit and
/// then drain the after-commit queue.
///
/// `ctx` is the [`WriteCtx`] the work runs under (actor, origin, scope,
/// per-caller options); `work` borrows the transaction only, so it captures
/// whatever it needs from `ctx`. A `work` error rolls the transaction back,
/// drops the queue without running it, and answers the work error.
///
/// The callback shape mirrors
/// [`sea_orm::TransactionTrait::transaction`]: a higher-ranked boxed future,
/// so the work can borrow the transaction it runs on.
pub async fn run<T, F>(
    ctx: &WriteCtx,
    db: &sea_orm::DatabaseConnection,
    work: F,
) -> Result<T, DomainError>
where
    F: for<'t> FnOnce(
            &'t mut WriteTxn,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<T, DomainError>> + Send + 't>,
        > + Send,
    T: Send,
{
    tracing::debug!(actor = %ctx.actor.user_id, "domain write started");
    let mut txn = WriteTxn::begin(db).await?;
    match work(&mut txn).await {
        Ok(value) => {
            txn.commit().await?;
            Ok(value)
        }
        Err(error) => {
            // The work error is the answer; a rollback failure on top of it
            // means the connection is already gone, so it only warns.
            if let Err(rollback) = txn.rollback().await {
                tracing::warn!(%rollback, "domain write rollback failed after work error");
            }
            Err(error)
        }
    }
}
