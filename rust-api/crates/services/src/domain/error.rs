//! Why a domain write failed.

use thiserror::Error as ThisError;

/// Why a domain write failed. Mirrors the [`pidash_db::tx::TxError`] shape:
/// a database failure, or the work itself declining.
#[derive(Debug, ThisError)]
pub enum DomainError {
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("work failed: {0}")]
    Work(String),
}
