//! Transaction wrapper with post-commit actions.
//!
//! Mirrors `transaction.atomic()` + `transaction.on_commit(...)`: the
//! codebase schedules side effects (task fan-out, cache invalidation,
//! webhook dispatch) to run only after the enclosing transaction commits.
//! Django signals become explicit [`Transaction::on_commit`] calls at the
//! write site; there is no implicit receiver list.
//!
//! The contract, as in Django:
//!
//! - Actions queued with `on_commit` run exactly once, in order, after a
//!   successful commit.
//! - On rollback — explicit or via drop, since sqlx rolls a live
//!   transaction back when it is dropped — queued actions are discarded
//!   and never run.

use sqlx::{PgPool, Postgres, Transaction as SqlxTransaction};
use thiserror::Error as ThisError;

/// A side effect deferred until the transaction commits.
///
/// `Send + 'static` so actions can move owned data (task payloads,
/// event rows) into background dispatch after commit.
pub type PostCommitAction = Box<dyn FnOnce() + Send + 'static>;

/// The deferred-action queue. Pure logic, unit-tested without a database.
#[derive(Default)]
pub struct AfterCommit {
    actions: Vec<PostCommitAction>,
}

impl AfterCommit {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue an action for after commit (Django's `on_commit`).
    /// Calling this outside a transaction runs the action immediately in
    /// Django; here there is no outside: actions live on a [`Transaction`].
    pub fn on_commit(&mut self, action: impl FnOnce() + Send + 'static) {
        self.actions.push(Box::new(action));
    }

    /// Run every queued action in order. Consumes the queue, so actions
    /// run at most once even if `commit` were called twice.
    pub fn commit(mut self) {
        for action in self.actions.drain(..) {
            action();
        }
    }

    pub fn len(&self) -> usize {
        self.actions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

/// Why a transactional unit of work failed.
#[derive(Debug, ThisError)]
pub enum TxError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("work failed: {0}")]
    Work(String),
}

/// A sqlx transaction plus its post-commit queue.
pub struct Transaction<'c> {
    inner: Option<SqlxTransaction<'c, Postgres>>,
    after: AfterCommit,
}

impl<'c> Transaction<'c> {
    pub async fn begin(pool: &'c PgPool) -> Result<Self, sqlx::Error> {
        Ok(Self {
            inner: Some(pool.begin().await?),
            after: AfterCommit::new(),
        })
    }

    /// Direct access to the sqlx transaction for query execution.
    pub fn inner(&mut self) -> &mut SqlxTransaction<'c, Postgres> {
        self.inner.as_mut().expect("transaction already finished")
    }

    pub fn on_commit(&mut self, action: impl FnOnce() + Send + 'static) {
        self.after.on_commit(action);
    }

    /// Commit, then run the queued actions in order. Actions never run
    /// when the commit itself fails.
    pub async fn commit(mut self) -> Result<(), sqlx::Error> {
        let inner = self.inner.take().expect("transaction already finished");
        inner.commit().await?;
        self.after.commit();
        Ok(())
    }

    /// Roll back and discard the queued actions without running them.
    pub async fn rollback(mut self) -> Result<(), sqlx::Error> {
        let inner = self.inner.take().expect("transaction already finished");
        inner.rollback().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn committed_actions_run_in_order_exactly_once() {
        let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let mut after = AfterCommit::new();
        for step in ["first", "second"] {
            let log = Arc::clone(&log);
            after.on_commit(move || log.lock().expect("lock").push(step));
        }
        assert_eq!(after.len(), 2);
        after.commit();
        assert_eq!(*log.lock().expect("lock"), vec!["first", "second"]);
    }

    #[test]
    fn dropped_queue_runs_nothing() {
        let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        {
            let mut after = AfterCommit::new();
            let log = Arc::clone(&log);
            after.on_commit(move || log.lock().expect("lock").push("lost"));
            drop(after);
        }
        assert!(log.lock().expect("lock").is_empty());
    }

    #[test]
    fn queue_starts_empty() {
        assert!(AfterCommit::new().is_empty());
    }
}
