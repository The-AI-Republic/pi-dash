//! The shared typed core (NEWPIDASH-190 §4.3–4.4): one implementation
//! over a caller-supplied [`WriteTxn`](crate::domain::WriteTxn) that
//! the operations layer and the cloud-agent tools call over their own
//! transactions. Verdicts and renderings stay with the wrappers; the
//! writes here equal the domain row write the wrappers used to call
//! privately, so wire-protocol and result-kind behavior is
//! structurally unchanged.

pub mod workpad;

use std::fmt;

use crate::domain::DomainError;

/// A domain-write failure carrying the domain error type.
#[derive(Debug)]
pub enum CoreError {
    /// A domain failure (SeaORM fetch/insert/update errors).
    Db(DomainError),
    /// A sqlx failure from the row-level domain writes that return
    /// sqlx-native errors directly.
    Sqlx(sqlx::Error),
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Db(error) => write!(f, "database error: {error}"),
            Self::Sqlx(error) => write!(f, "database error: {error}"),
        }
    }
}

impl std::error::Error for CoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Db(error) => Some(error),
            Self::Sqlx(error) => Some(error),
        }
    }
}

impl From<DomainError> for CoreError {
    fn from(error: DomainError) -> Self {
        Self::Db(error)
    }
}

impl From<sqlx::Error> for CoreError {
    fn from(error: sqlx::Error) -> Self {
        Self::Sqlx(error)
    }
}
