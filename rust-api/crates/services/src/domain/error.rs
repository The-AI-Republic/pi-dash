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

impl DomainError {
    /// A hook's dangling-row miss (`DbErr::RecordNotFound`) — the drivers'
    /// `NotFound` arm (a dangling state FK on save).
    pub fn is_not_found(&self) -> bool {
        matches!(self, DomainError::Db(sea_orm::DbErr::RecordNotFound(_)))
    }

    /// A Postgres integrity violation (`23xxx`) through the SeaORM wrap —
    /// the drivers' `"The payload is not valid"` arm. Mirrors the
    /// handlers' `is_integrity_violation` over the unwrapped sqlx error.
    pub fn is_integrity_violation(&self) -> bool {
        match self {
            DomainError::Db(sea_orm::DbErr::Exec(sea_orm::RuntimeErr::SqlxError(
                sqlx::Error::Database(db),
            ))) => db.code().is_some_and(|code| code.starts_with("23")),
            _ => false,
        }
    }
}
