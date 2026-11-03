#![forbid(unsafe_code)]

//! HTTP denials for the F-06 permission kernel.
//!
//! The `@allow_permission` decorator, view-inline 403s, and DRF permission
//! denials answer as compact JSON rendered by `JSONRenderer` with the
//! project defaults (`COMPACT_JSON=True`, `UNICODE_JSON=True` in
//! `rest_framework/settings.py`, so separators are `SHORT_SEPARATORS =
//! (',', ':')` from `rest_framework/compat.py` — no spaces). The bodies
//! below are those exact bytes:
//!
//! - allow-style denial (the decorator in `app/permissions/base.py` and its
//!   `utils` copy, plus view-inline 403s such as
//!   `app/views/project/base.py`):
//!   `{"error":"You don't have the required permissions."}`
//! - DRF-default denial: permission classes without a `message` attribute
//!   (every class in `app/permissions/` except the desktop gate) deny
//!   through `APIView.permission_denied` with `message=None`, which raises
//!   `PermissionDenied` with its default detail:
//!   `{"detail":"You do not have permission to perform this action."}`
//! - desktop gate (`IsDesktopSession.message`, key order `error`, `detail`):
//!   `{"error":"desktop_session_required","detail":"This endpoint is
//!   available to the Pi Dash desktop app."}`
//!
//! The bodies are string constants on purpose: building them through
//! `serde_json::json!` would re-sort object keys alphabetically (`detail`
//! before `error`) and break the byte parity. [`require_allow`] maps a
//! kernel decision to the allow-style denial; domain handlers fetch their
//! membership rows through the [`ScopedWrites`](pidash_db::context::ScopedWrites)
//! handle and decide through `pidash_auth::permissions`.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

/// Exact bytes of the `@allow_permission` / view-inline 403 body.
pub const PERMISSION_DENIED_BODY: &str = r#"{"error":"You don't have the required permissions."}"#;
/// Exact bytes of DRF's default permission-denied body: what the
/// `app/permissions/` classes (no `message` attribute) render through
/// `APIView.permission_denied` with `message=None`.
pub const DEFAULT_DENIED_BODY: &str =
    r#"{"detail":"You do not have permission to perform this action."}"#;
/// Exact bytes of the `IsDesktopSession` 403 body (key order preserved).
pub const DESKTOP_REQUIRED_BODY: &str = r#"{"error":"desktop_session_required","detail":"This endpoint is available to the Pi Dash desktop app."}"#;

fn json_forbidden(body: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::FORBIDDEN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("static permission-denied response")
}

/// Rejection for a failed permission check: answers the allow-style 403.
#[derive(Debug, Clone, Copy, Default)]
pub struct PermissionDenied;

impl IntoResponse for PermissionDenied {
    fn into_response(self) -> Response {
        json_forbidden(PERMISSION_DENIED_BODY)
    }
}

/// Rejection for a denied permission-class check: answers the DRF-default
/// 403 (`VIEWSET_FORBIDDEN` in the contract suites).
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultPermissionDenied;

impl IntoResponse for DefaultPermissionDenied {
    fn into_response(self) -> Response {
        json_forbidden(DEFAULT_DENIED_BODY)
    }
}

/// Rejection for the desktop-only gate: answers the 403 with the
/// `desktop_session_required` code so clients can tell "not signed in"
/// apart from "not for browsers".
#[derive(Debug, Clone, Copy, Default)]
pub struct DesktopSessionRequired;

impl IntoResponse for DesktopSessionRequired {
    fn into_response(self) -> Response {
        json_forbidden(DESKTOP_REQUIRED_BODY)
    }
}

/// Map an `pidash_auth::permissions` decision to the allow-style denial:
/// `true` passes the request through, `false` answers the exact 403.
pub fn require_allow(allowed: bool) -> Result<(), PermissionDenied> {
    if allowed {
        Ok(())
    } else {
        Err(PermissionDenied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body_of(response: Response) -> (StatusCode, String, Option<String>) {
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok().map(str::to_owned));
        let bytes = to_bytes(response.into_body(), 1024)
            .await
            .expect("read body");
        (
            status,
            String::from_utf8(bytes.to_vec()).expect("utf-8"),
            content_type,
        )
    }

    #[tokio::test]
    async fn allow_denial_is_byte_identical() {
        let (status, body, content_type) = body_of(PermissionDenied.into_response()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body,
            r#"{"error":"You don't have the required permissions."}"#
        );
        assert_eq!(body, PERMISSION_DENIED_BODY);
        assert_eq!(content_type.as_deref(), Some("application/json"));
    }

    #[tokio::test]
    async fn default_denial_matches_drf_permission_denied() {
        let (status, body, content_type) = body_of(DefaultPermissionDenied.into_response()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        // DRF `PermissionDenied.default_detail`, compact separators.
        assert_eq!(
            body,
            r#"{"detail":"You do not have permission to perform this action."}"#
        );
        assert_eq!(body, DEFAULT_DENIED_BODY);
        assert_eq!(content_type.as_deref(), Some("application/json"));
    }

    #[tokio::test]
    async fn desktop_denial_keeps_key_order() {
        let (status, body, _) = body_of(DesktopSessionRequired.into_response()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        // `error` before `detail`: insertion order, not alphabetical.
        assert!(body.find("\"error\"").unwrap() < body.find("\"detail\"").unwrap());
        assert_eq!(body, DESKTOP_REQUIRED_BODY);
    }

    #[test]
    fn require_allow_maps_decisions() {
        assert!(require_allow(true).is_ok());
        let denial = require_allow(false).unwrap_err();
        let response = denial.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
