//! The operation outcome: a status plus the pre-rendered body text.
//!
//! Operations return the bytes the caller sends, never an HTTP type: the
//! v1 handler wraps them in its response, and non-HTTP callers (the MCP
//! tools) read them directly.
//!
//! The variants are driven by what the operations actually construct.
//! Body parsing (malformed-JSON 400s, 415s, the 5 MiB 413) stays
//! handler-side as request transport, so no variant covers it; auth and
//! the gate stay handler-side too. Two spellings need care:
//!
//! - The `{"error":…}` view-inline 400 ([`Error::BadError`]) and the
//!   `{"error":"The requested resource does not exist."}` 404
//!   ([`Error::NotFound`]) are spelled identically on every v1
//!   work-items endpoint, so one variant each covers them all.
//! - The DRF `detail` envelope is NOT shared: the issues family answers
//!   `{"Detail":…}` (uppercase) while move/search answers `{"detail":…}`
//!   (lowercase). No operation constructs a bare detail envelope, so no
//!   variant covers it either — [`Error::ProjectNotFound`] carries the
//!   move's lowercase `{"detail":"Project not found"}` whole.
//!
//! Each handler module maps [`Error`] onto its own `Denial` with a `From`
//! impl: variants its operations return map faithfully; arms for variants
//! its operations never return map to the 500, which signals the wiring
//! bug loudly instead of rendering a wrong body silently.

/// A successful operation: the status code plus the rendered body text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The HTTP status code (200 for updates, 201 for creates).
    pub status: u16,
    /// The pre-rendered response body text.
    pub body: String,
}

impl Outcome {
    pub fn ok(body: String) -> Self {
        Self { status: 200, body }
    }

    pub fn created(body: String) -> Self {
        Self { status: 201, body }
    }
}

/// An operation failure: the status code plus the pre-rendered body text.
///
/// The `String` payloads are the exact bodies the v1 handlers answered
/// before the move; [`Error::status_and_body`] pairs each with its status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// 400, `{"error":…}` (view-inline: unknown timezones,
    /// override-detail failures).
    BadError(String),
    /// 400, serializer `errors` dict (pre-rendered bytes, field order).
    FieldErrors(String),
    /// 404, `{"error":"The requested resource does not exist."}`.
    NotFound,
    /// 404, view-inline `{"error": …}` with the full body.
    NotFoundBody(String),
    /// 404, `{"detail":"Project not found"}` (the move's target-project
    /// miss — lowercase `Detail`, as the move endpoints answer it).
    ProjectNotFound,
    /// 409, view-inline body (external duplicate).
    Conflict(String),
    /// The move's `IssueMoveError` mapping: `{"error":message}` with a
    /// dynamic status (400/403/409).
    Rejection { status: u16, message: String },
    /// 500, generic branch.
    ServerError,
}

impl Error {
    /// The HTTP status code plus the response body text.
    pub fn status_and_body(&self) -> (u16, String) {
        match self {
            Error::BadError(message) => (400, error_body(message)),
            Error::FieldErrors(body) => (400, body.clone()),
            Error::NotFound => (404, RESOURCE_NOT_FOUND_BODY.to_owned()),
            Error::NotFoundBody(body) => (404, body.clone()),
            Error::ProjectNotFound => (404, PROJECT_NOT_FOUND_BODY.to_owned()),
            Error::Conflict(body) => (409, body.clone()),
            Error::Rejection { status, message } => (*status, error_body(message)),
            Error::ServerError => (500, pidash_compat::response::SERVER_ERROR_BODY.to_owned()),
        }
    }
}

/// `{"Detail":"Project not found"}` (the move's target-project miss —
/// lowercase `Detail`, as the move endpoints answer it).
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;

/// `{"error":"The payload is not valid"}` (the port's invalid-payload
/// 400, answered where Django's `handle_exception` catches the
/// `IntegrityError`).
pub const PAYLOAD_NOT_VALID_BODY: &str = r#"{"error":"The payload is not valid"}"#;

/// `{"error":"The requested resource does not exist."}` (the port's
/// missing-row 404).
pub const RESOURCE_NOT_FOUND_BODY: &str = r#"{"error":"The requested resource does not exist."}"#;

/// `{"error":"Please provide valid detail"}` (the override-detail 400).
pub const VALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;

pub(crate) fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

fn error_body(message: &str) -> String {
    format!("{{\"error\":{}}}", json_string(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_and_body_renders_exact_bytes() {
        assert_eq!(
            Error::BadError("nope".to_owned()).status_and_body(),
            (400, r#"{"error":"nope"}"#.to_owned())
        );
        assert_eq!(
            Error::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned()).status_and_body(),
            (400, PAYLOAD_NOT_VALID_BODY.to_owned())
        );
        assert_eq!(
            Error::NotFound.status_and_body(),
            (404, RESOURCE_NOT_FOUND_BODY.to_owned())
        );
        assert_eq!(
            Error::NotFoundBody(RESOURCE_NOT_FOUND_BODY.to_owned()).status_and_body(),
            (404, RESOURCE_NOT_FOUND_BODY.to_owned())
        );
        assert_eq!(
            Error::ProjectNotFound.status_and_body(),
            (404, PROJECT_NOT_FOUND_BODY.to_owned())
        );
        assert_eq!(
            Error::Conflict("dup".to_owned()).status_and_body(),
            (409, "dup".to_owned())
        );
        assert_eq!(
            Error::Rejection {
                status: 409,
                message: "boom".to_owned(),
            }
            .status_and_body(),
            (409, r#"{"error":"boom"}"#.to_owned())
        );
        let (status, body) = Error::ServerError.status_and_body();
        assert_eq!(status, 500);
        assert_eq!(body, pidash_compat::response::SERVER_ERROR_BODY);
    }

    #[test]
    fn outcome_constructors_set_status() {
        assert_eq!(Outcome::ok("b".to_owned()).status, 200);
        assert_eq!(Outcome::created("b".to_owned()).status, 201);
    }
}
