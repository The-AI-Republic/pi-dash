//! The single error type every layer returns.
//!
//! The mapping to HTTP status codes is a pure number on purpose: the axum
//! layer in `pidash-api` turns an [`Error`] into a response, so this crate
//! stays free of HTTP types.

use thiserror::Error as ThisError;

/// Every failure the Rust backend can report to a caller.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum Error {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("unauthenticated")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("not found: {0}")]
    NotFound(String),
    #[error("internal error")]
    Internal,
}

impl Error {
    /// HTTP status code for this error, as a plain number.
    pub fn status_code(&self) -> u16 {
        match self {
            Error::BadRequest(_) => 400,
            Error::Unauthorized => 401,
            Error::Forbidden => 403,
            Error::NotFound(_) => 404,
            Error::Internal => 500,
        }
    }

    /// Stable machine-readable key for the JSON `error` object.
    pub fn error_key(&self) -> &'static str {
        match self {
            Error::BadRequest(_) => "bad_request",
            Error::Unauthorized => "unauthenticated",
            Error::Forbidden => "permission_denied",
            Error::NotFound(_) => "not_found",
            Error::Internal => "internal_error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_match_django_rest_framework() {
        assert_eq!(Error::BadRequest("x".into()).status_code(), 400);
        assert_eq!(Error::Unauthorized.status_code(), 401);
        assert_eq!(Error::Forbidden.status_code(), 403);
        assert_eq!(Error::NotFound("x".into()).status_code(), 404);
        assert_eq!(Error::Internal.status_code(), 500);
    }

    #[test]
    fn error_keys_are_stable() {
        assert_eq!(Error::Forbidden.error_key(), "permission_denied");
        assert_eq!(Error::Unauthorized.error_key(), "unauthenticated");
        assert_eq!(Error::NotFound("i".into()).error_key(), "not_found");
    }
}
