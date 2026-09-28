//! Shared D-01 license handler base (stage 3).
//!
//! Port of `apps/api/pi_dash/license/api/views/base.py`, the base every
//! license handler reuses (`admin.py` and `configuration.py` import it;
//! `instance.py` uses the identical `pi_dash.app.views.BaseAPIView`, and
//! `workspace.py` the identical `pi_dash.app.views.base.BaseAPIView` — the
//! `handle_exception` / `dispatch` / `fields` / `expand` bodies are the
//! same; only `BaseViewSet` adds a traceback print this domain never
//! uses):
//!
//! * `TimezoneMixin.initial` (`base.py:34-39`) — [`resolve_request_tz`].
//! * `BaseAPIView` defaults (`base.py:42-51`) — the constants below.
//! * `filter_queryset` loop (`base.py:53-56`) — [`filter_is_identity`].
//! * `handle_exception` matrix (`base.py:58-95`) — [`map_exception`].
//! * `dispatch` (`base.py:97-109`) — documented quirk, see below.
//! * `fields` / `expand` (`base.py:111-119`) — [`csv_param`].
//!
//! Dispatch quirk (ported as-is, listed for follow-up): the `except` path
//! computes `response = self.handle_exception(exc)` and then returns `exc`
//! instead of `response` (`base.py:107-109`). It is unreachable: the
//! `handle_exception` override catches `Exception` and always returns a
//! `Response`, never re-raising, so nothing escapes DRF's dispatch into
//! that `except`. No wire effect; the port returns the mapped response.
//!
//! The exception matrix, `fields`/`expand` cases and timezone branches are
//! pinned by `rust-api/fixtures/license/handlers/base.golden.json` and
//! replayed byte-identical by the tests below.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

// ---------------------------------------------------------------------------
// BaseAPIView defaults (base.py:42-51)
// ---------------------------------------------------------------------------

/// `permission_classes = [InstanceAdminPermission]` (`base.py:43`):
/// deny-by-default; endpoints opt out explicitly (`AllowAny`, ...).
pub const DEFAULT_PERMISSION: &str = "InstanceAdminPermission";

/// `authentication_classes = [BaseSessionAuthentication]` (`base.py:47`):
/// Django session auth without CSRF enforcement.
pub const AUTHENTICATION_CLASS: &str = "BaseSessionAuthentication";

/// `filter_backends = (DjangoFilterBackend, SearchFilter)` (`base.py:45`).
pub const FILTER_BACKENDS: &[&str] = &["DjangoFilterBackend", "SearchFilter"];

/// `filterset_fields = []` (`base.py:49`).
pub const FILTERSET_FIELDS: &[&str] = &[];
/// `search_fields = []` (`base.py:51`).
pub const SEARCH_FIELDS: &[&str] = &[];

/// `filter_queryset` (`base.py:53-56`) applies each backend in order. With
/// the D-01 defaults above both backends are identity (empty field lists
/// filter nothing), so the loop result equals its input.
pub fn filter_is_identity() -> bool {
    FILTERSET_FIELDS.is_empty() && SEARCH_FIELDS.is_empty()
}

// ---------------------------------------------------------------------------
// TimezoneMixin (base.py:28-39)
// ---------------------------------------------------------------------------

/// Resolve the request's rendering zone (`TimezoneMixin.initial`).
///
/// * anonymous → `timezone.deactivate()`: the default zone, `TIME_ZONE =
///   "UTC"` (`settings/common.py:362`), so [`chrono_tz::UTC`];
/// * authenticated → `timezone.activate(ZoneInfo(user_timezone))`
///   (`base.py:36-37`); an unknown zone (or a missing one — `ZoneInfo(None)`
///   raises) is a 500 through the `handle_exception` else-branch.
pub fn resolve_request_tz(
    authenticated: bool,
    user_timezone: Option<&str>,
) -> Result<chrono_tz::Tz, HandlerError> {
    if !authenticated {
        return Ok(chrono_tz::UTC);
    }
    match user_timezone {
        Some(name) => name
            .parse::<chrono_tz::Tz>()
            .map_err(|_| HandlerError::ServerError),
        None => Err(HandlerError::ServerError),
    }
}

// ---------------------------------------------------------------------------
// handle_exception matrix (base.py:58-95) + DRF-inherited denials
// ---------------------------------------------------------------------------

/// Exact bytes of the DRF `IsAuthenticated` denial: anonymous on a guarded
/// route. (`APIView.permission_denied` with `message=None`.)
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `handle_exception`'s `IntegrityError` branch (`base.py:67-71`).
pub const INVALID_PAYLOAD_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// `handle_exception`'s `ValidationError` branch (`base.py:73-77`).
pub const INVALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch (`base.py:79-83`).
pub const NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// `handle_exception`'s `KeyError` branch (`base.py:85-89`).
pub const MISSING_KEY_BODY: &str = r#"{"error":"The required key does not exist."}"#;
/// `handle_exception`'s generic branch (`base.py:91-95`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// DRF-default permission denial for an authenticated user the
/// `InstanceAdminPermission` rejects (the class sets no `message`, so
/// `permission_denied` raises `PermissionDenied` with its default detail).
/// Shared with [`crate::permissions::DEFAULT_DENIED_BODY`].
pub const FORBIDDEN_BODY: &str = crate::permissions::DEFAULT_DENIED_BODY;

/// `GET /api/instances/` answers `Cache-Control: private, max-age=12`
/// (`@cache_control(private=True, max_age=12)`, `instance.py:35`; exact
/// bytes verified against Django).
pub const CACHE_CONTROL_VALUE: &str = "private, max-age=12";

/// Handler failure with its exact status + body: the `handle_exception`
/// matrix plus the DRF denials the license views inherit via `super()`.
#[derive(Debug)]
pub enum HandlerError {
    /// 401, DRF `NotAuthenticated` (no session on a guarded route).
    Unauthorized,
    /// 403, DRF-default denial (authed non-admin).
    Forbidden,
    /// 400, `IntegrityError` branch.
    InvalidPayload,
    /// 400, `ValidationError` branch.
    InvalidDetail,
    /// 404, `ObjectDoesNotExist` branch.
    NotFound,
    /// 400, `KeyError` branch.
    MissingKey,
    /// 400, serializer `errors` dict (PATCH invalid branch).
    FieldErrors(String),
    /// 400, view-inline `{"error": ...}` bodies.
    BadError(String),
    /// 400, DRF `{"detail": ...}` bodies (parse errors, ...).
    BadDetail(String),
    /// 500, generic branch (logged, like `log_exception`).
    ServerError,
}

impl HandlerError {
    pub fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            HandlerError::Unauthorized => {
                (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned())
            }
            HandlerError::Forbidden => (StatusCode::FORBIDDEN, FORBIDDEN_BODY.to_owned()),
            HandlerError::InvalidPayload => {
                (StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY.to_owned())
            }
            HandlerError::InvalidDetail => {
                (StatusCode::BAD_REQUEST, INVALID_DETAIL_BODY.to_owned())
            }
            HandlerError::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
            HandlerError::MissingKey => (StatusCode::BAD_REQUEST, MISSING_KEY_BODY.to_owned()),
            HandlerError::FieldErrors(body) | HandlerError::BadError(body) => {
                (StatusCode::BAD_REQUEST, body.clone())
            }
            HandlerError::BadDetail(body) => (StatusCode::BAD_REQUEST, body.clone()),
            HandlerError::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }

    /// Map a Python exception kind to the `handle_exception` response
    /// (`base.py:63-95`): the `try: super().handle_exception(exc)` line
    /// handles DRF's own exceptions; the `except` maps Django errors by
    /// type in order, falling through to log + 500.
    pub fn from_exception_kind(kind: ExceptionKind) -> Self {
        match kind {
            ExceptionKind::IntegrityError => HandlerError::InvalidPayload,
            ExceptionKind::ValidationError => HandlerError::InvalidDetail,
            ExceptionKind::ObjectDoesNotExist => HandlerError::NotFound,
            ExceptionKind::KeyError => HandlerError::MissingKey,
            ExceptionKind::Other => HandlerError::ServerError,
        }
    }
}

/// The exception types `handle_exception` discriminates on (`base.py:67-91`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionKind {
    IntegrityError,
    ValidationError,
    ObjectDoesNotExist,
    KeyError,
    Other,
}

impl IntoResponse for HandlerError {
    fn into_response(self) -> Response {
        if matches!(self, HandlerError::ServerError) {
            // `log_exception(e)` (`base.py:91`).
            tracing::warn!("license handler: internal error");
        }
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("static handler-error response")
    }
}

/// Render `body` (already exact JSON bytes) as a JSON response.
pub fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("handler json response")
}

// ---------------------------------------------------------------------------
// fields / expand (base.py:111-119)
// ---------------------------------------------------------------------------

/// Decode one `application/x-www-form-urlencoded` value (`+` → space,
/// `%XX` → byte; malformed sequences pass through like Django's forgiving
/// unquote).
fn urldecode(raw: &str) -> String {
    let mut out = Vec::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(hi), Some(lo)) => {
                        out.push(hi * 16 + lo);
                        i += 3;
                    }
                    _ => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `fields` / `expand` query-param properties (`base.py:111-119`):
/// `request.GET.get(key, "")` (last value wins on repeats), split on
/// `,`, drop empties, `None` when nothing remains.
pub fn csv_param(raw_query: &str, key: &str) -> Option<Vec<String>> {
    let mut last: Option<&str> = None;
    for pair in raw_query.split('&') {
        let (name, value) = match pair.split_once('=') {
            Some((n, v)) => (n, v),
            None => (pair, ""),
        };
        if urldecode(name) == key {
            last = Some(value);
        }
    }
    let items: Vec<String> = last
        .map(urldecode)
        .unwrap_or_default()
        .split(',')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect();
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn golden() -> serde_json::Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/license/handlers/base.golden.json");
        let text = std::fs::read_to_string(&path).expect("base golden exists");
        serde_json::from_str(&text).expect("base golden is valid JSON")
    }

    fn compact(value: &serde_json::Value) -> String {
        serde_json::to_string(value).expect("compact json")
    }

    #[test]
    fn exception_matrix_replays_golden_byte_identical() {
        let gold = golden();
        let rows = gold["exception_matrix"]
            .as_array()
            .expect("matrix is an array");
        let kinds = [
            ExceptionKind::IntegrityError,
            ExceptionKind::ValidationError,
            ExceptionKind::ObjectDoesNotExist,
            ExceptionKind::KeyError,
            ExceptionKind::Other,
        ];
        assert_eq!(rows.len(), kinds.len(), "matrix covers every kind");
        for (row, kind) in rows.iter().zip(kinds) {
            let error = HandlerError::from_exception_kind(kind);
            let (status, body) = error.status_and_body();
            assert_eq!(status.as_u16(), row["status"].as_u64().unwrap() as u16);
            assert_eq!(body, compact(&row["body"]), "kind {kind:?}");
        }
    }

    #[test]
    fn fields_expand_cases_replay_golden() {
        let gold = golden();
        for case in gold["fields_expand"]["cases"].as_array().unwrap() {
            let raw = case["in"].as_str().unwrap();
            let got = csv_param(raw, "fields");
            match case["out"].clone() {
                serde_json::Value::Null => assert_eq!(got, None, "input {raw:?}"),
                serde_json::Value::Array(items) => {
                    let want: Vec<String> = items
                        .iter()
                        .map(|v| v.as_str().unwrap().to_owned())
                        .collect();
                    assert_eq!(got, Some(want), "input {raw:?}");
                }
                other => panic!("unexpected golden out: {other}"),
            }
        }
        // Repeats: Django QueryDict.get returns the LAST value.
        assert_eq!(
            csv_param("fields=a&fields=b,c", "fields"),
            Some(vec!["b".to_owned(), "c".to_owned()])
        );
        assert_eq!(csv_param("expand=x", "fields"), None);
    }

    #[test]
    fn timezone_branches_match_golden() {
        assert_eq!(
            resolve_request_tz(false, None).expect("anon"),
            chrono_tz::UTC
        );
        assert_eq!(
            resolve_request_tz(false, Some("America/New_York")).expect("anon ignores zone"),
            chrono_tz::UTC
        );
        let tz = resolve_request_tz(true, Some("America/New_York")).expect("valid zone");
        assert_eq!(tz, chrono_tz::America::New_York);
        assert!(matches!(
            resolve_request_tz(true, Some("Not/AZone")),
            Err(HandlerError::ServerError)
        ));
        assert!(matches!(
            resolve_request_tz(true, None),
            Err(HandlerError::ServerError)
        ));
    }

    #[test]
    fn base_defaults_match_python() {
        assert_eq!(DEFAULT_PERMISSION, "InstanceAdminPermission");
        assert_eq!(AUTHENTICATION_CLASS, "BaseSessionAuthentication");
        assert_eq!(FILTER_BACKENDS, &["DjangoFilterBackend", "SearchFilter"]);
        assert!(FILTERSET_FIELDS.is_empty());
        assert!(SEARCH_FIELDS.is_empty());
        assert!(filter_is_identity());
        assert_eq!(CACHE_CONTROL_VALUE, "private, max-age=12");
    }

    #[test]
    fn denial_bodies_are_exact() {
        assert_eq!(
            HandlerError::Unauthorized.status_and_body().1,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            HandlerError::Forbidden.status_and_body().1,
            r#"{"detail":"You do not have permission to perform this action."}"#
        );
    }
}
