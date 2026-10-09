//! Runner enrollment/auth/machine API layer (D-13, `api/v1/runner/`).
//!
//! This module is the authentication/throttle shelf the D-13 handler ports
//! build on (PIDASHCONV-589):
//!
//! - [`auth`]: the `runner/authentication.py` extractor ports (access-token,
//!   refresh-token, machine-token) plus the consumed `APIKeyAuthentication`
//!   behaviour — fixture id D13-F4.
//! - [`throttle`]: the inherited DRF `AnonRateThrottle` (30/minute) for the
//!   enroll, redeem, and health endpoints — fixture ids D13-F4 + D13-F7.
//!
//! - [`desktop`]: the desktop-app machine enrollment endpoint
//!   (`runner/views/desktop.py`, PIDASHCONV-595) — fixture ids D13-F5 +
//!   D13-F6 + D13-F7.
//! - [`projects`]: the runner project list (`runner/views/projects.py`,
//!   PIDASHCONV-595) — fixture ids D13-F2 + D13-F5 + D13-F7.
//! - [`enroll`]: the daemon enrollment handlers (PIDASHCONV-590) —
//!   enroll, create, ticket, redeem, health, invite/revive 410s —
//!   fixture ids D13-F2 + D13-F3 + D13-F4 + D13-F5 + D13-F7.
//!
//! Kernel reuse only (`pidash_auth`, `pidash_types`), plus this module's own
//! SQL: no cross-domain code dependency.
//!
//! - [`manage`]: the web runners/machines/pods endpoints
//!   (`runner/views/runners.py:62-125,289-425`, `runner/views/pods.py`,
//!   PIDASHCONV-591) — fixture ids D13-F2 + D13-F5 + D13-F6 + D13-F7.
//!
//! - [`delete_cmds`]: the deletes + machine-command endpoints
//!   (`runner/views/runners.py:249-286,427-452`,
//!   `runner/views/machine_commands.py:63-257`, PIDASHCONV-593) —
//!   fixture ids D13-F5 + D13-F6 + D13-F7 + D13-F8.
//!
//! The shared request-body layer lives here ([`read_request_data`] and the
//! `data_*`/`or_empty_*` field helpers) so both handlers parse `request.data`
//! identically; sibling handler issues add their own `pub mod` lines above,
//! keeping both sides on conflict.

pub mod auth;
pub mod delete_cmds;
pub mod desktop;
pub mod enroll;
pub mod manage;
pub mod projects;
pub mod teardown;
pub mod throttle;

use axum::extract::Request;
use axum::extract::State;
use axum::http::header;
use axum::http::StatusCode;
use axum::response::Response;
use chrono::DateTime;
use chrono::Utc;
use http_body_util::BodyExt as _;
use uuid::Uuid;

use crate::runner_runs::json_response;
use crate::runner_runs::server_error;
use crate::state::AppState;
use crate::v1_cycles_modules::json_cpython;
use crate::v1_cycles_modules::json_cpython::JObject;
use crate::v1_cycles_modules::json_cpython::JVal;
use crate::v1_cycles_modules::json_cpython::JsonFail;

// ---------------------------------------------------------------------------
// Request bodies (the `manage.rs` envelope over `json_cpython`, PIDASHCONV-591)
// ---------------------------------------------------------------------------

/// Read `request.data` for a POST/DELETE body: content-length 0 validates as
/// `{}` with the body ignored; a non-JSON content type proxies to Django
/// (form posts stay on the Python plane); unparsable JSON 400s with DRF's
/// `ParseError` (`{"detail": "JSON parse error - …"}`, lowercase key,
/// CPython message); past the depth cap is Django's JSON 500.
///
/// (The sibling `runner_runs::read_request_data` renders the 400 with a
/// capital-`D` key; Django lowercases it — verified against DRF 3.15.2 — so
/// this module owns its reader rather than inheriting the wrong bytes.)
#[allow(clippy::result_large_err)]
pub(crate) async fn read_request_data(state: &AppState, req: Request) -> Result<JVal, Response> {
    let content_length = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if content_length == 0 {
        return Ok(JVal::Object(JObject::new()));
    }
    let raw_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    // `parse_header_parameters`: the main type lowercases; parameters
    // are ignored for parser selection (`_MediaType.match`).
    let main = raw_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if main != "application/json" {
        return Err(crate::edge::proxy(State(state.clone()), req).await);
    }
    let (_parts, body) = req.into_parts();
    let bytes = body
        .collect()
        .await
        .map(|collected| collected.to_bytes())
        .map_err(|_| server_error())?;
    match json_cpython::parse_request_data(&bytes) {
        Ok(value) => Ok(value),
        Err(JsonFail::Message(detail)) => Err(json_response(
            StatusCode::BAD_REQUEST,
            parse_error_body(&detail),
        )),
        Err(JsonFail::Recursion) => Err(server_error()),
    }
}

/// DRF `ParseError` body (`rest_framework/parsers.py` + the
/// `exception_handler` wrap): lowercase `detail`, the `JSON parse error - `
/// prefix, CPython's message. Verified byte-for-byte against DRF 3.15.2.
pub(crate) fn parse_error_body(detail: &str) -> String {
    format!(
        "{{\"{}\":{}}}",
        "detail",
        serde_json::to_string(&format!("{}{detail}", json_cpython::JSON_PARSE_PREFIX))
            .expect("json string"),
    )
}

/// Python truthiness over a parsed value (`None`/`False`/`0`/`""`/`[]`/`{}`
/// are falsy; everything else is truthy). Note `bool("false")` is `True`,
/// and `-0.0`/underflows are falsy.
pub(crate) fn j_truthy(value: &JVal) -> bool {
    match value {
        JVal::Null => false,
        JVal::Bool(flag) => *flag,
        JVal::Num(number) => !number.is_zero(),
        JVal::Str(text) => !text.is_empty(),
        JVal::Array(items) => !items.is_empty(),
        JVal::Object(map) => !map.is_empty(),
    }
}

/// `request.data.get(key)`: dict lookup — `None` for a missing key; any
/// non-object is Python's `AttributeError` → 500 (verified: arrays,
/// strings, and numbers all raise on `.get`).
#[allow(clippy::result_large_err)]
pub(crate) fn data_get<'a>(data: &'a JVal, key: &str) -> Result<Option<&'a JVal>, Response> {
    match data {
        JVal::Object(map) => Ok(map.get(key)),
        JVal::Null | JVal::Bool(_) | JVal::Num(_) | JVal::Str(_) | JVal::Array(_) => {
            Err(server_error())
        }
    }
}

/// Python `str.strip()` parity (the services-layer `py_strip` twin —
/// services is read-only from this crate's layer boundary, so the predicate
/// is mirrored, not imported): Python strips `str.isspace()` — Unicode
/// `White_Space` plus U+001C-U+001F and U+0085 — while Rust `trim()` strips
/// `White_Space` only.
pub(crate) fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| {
        c.is_whitespace() || c == '\u{85}' || ('\u{1c}'..='\u{1f}').contains(&c)
    })
}

/// `(request.data.get(key) or "").strip()`: falsy maps to `""`, strings
/// strip, truthy non-strings are the source's `AttributeError` → 500.
/// Surrogate-carrying strings flow through the lossy spelling — use only
/// where the value is compared, never bound (a surrogate is one
/// non-whitespace char that fails every downstream check identically).
#[allow(clippy::result_large_err)]
pub(crate) fn or_empty_lossy(value: Option<&JVal>) -> Result<String, Response> {
    let Some(value) = value else {
        return Ok(String::new());
    };
    if !j_truthy(value) {
        return Ok(String::new());
    }
    match value {
        JVal::Str(text) => Ok(py_strip(&text.to_lossy_string()).to_owned()),
        JVal::Null | JVal::Bool(_) | JVal::Num(_) | JVal::Array(_) | JVal::Object(_) => {
            Err(server_error())
        }
    }
}

/// [`or_empty_lossy`] for values bound into SQL or stored: a
/// surrogate-carrying string is Django's `UnicodeEncodeError` at the
/// Postgres encode → 500. Call at the bind site, in source order, so a
/// pure-Python guard ahead of it (the 409 version floor) still wins.
#[allow(clippy::result_large_err)]
pub(crate) fn or_empty_clean(value: Option<&JVal>) -> Result<String, Response> {
    let Some(value) = value else {
        return Ok(String::new());
    };
    if !j_truthy(value) {
        return Ok(String::new());
    }
    match value {
        JVal::Str(text) => text
            .to_clean_string()
            .map(|clean| py_strip(&clean).to_owned())
            .ok_or_else(server_error),
        JVal::Null | JVal::Bool(_) | JVal::Num(_) | JVal::Array(_) | JVal::Object(_) => {
            Err(server_error())
        }
    }
}

/// Django `request.query_params.get`: the last value wins on repeats
/// (`QueryDict`), and a missing or empty param disables the filter (every
/// source gate is an `if …:` truthiness check).
pub(crate) fn query_param(params: &crate::license::QueryMap, key: &str) -> Option<String> {
    crate::license::query_last(params, key).filter(|raw| !raw.is_empty())
}

/// UUID-typed query/body strings: Django's `UUIDField.get_prep_value`
/// raises `ValidationError` (an unhandled 500) on garbage — the
/// `chat_sessions_list` precedent answers [`server_error`].
#[allow(clippy::result_large_err)]
pub(crate) fn parse_uuid(raw: &str) -> Result<Uuid, Response> {
    raw.parse().map_err(|_| server_error())
}

/// `timezone.now()` truncated to microseconds (Django datetimes are
/// microsecond-exact; Postgres would round stored nanos).
pub(crate) fn now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    DateTime::from_timestamp_micros(now.timestamp_micros()).expect("micros in range")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_body(json: &str) -> JVal {
        json_cpython::parse_request_bytes(json.as_bytes()).expect("test json parses")
    }

    /// The 400 `ParseError` body keeps DRF's lowercase `detail` key and
    /// the `JSON parse error - ` prefix (verified against DRF 3.15.2).
    #[test]
    fn parse_error_body_lowercase_key() {
        assert_eq!(
            parse_error_body("Expecting value: line 1 column 1 (char 0)"),
            "{\"detail\":\"JSON parse error - Expecting value: line 1 column 1 (char 0)\"}"
        );
        let parsed: serde_json::Value =
            serde_json::from_str(&parse_error_body("x")).expect("parses");
        assert!(parsed.get("detail").is_some());
        assert!(parsed.get("Detail").is_none(), "lowercase key only");
    }

    /// `.get` on non-objects 500s; objects look up (missing → `None`).
    #[test]
    fn data_get_non_dict_500s() {
        for raw in ["[1]", "\"s\"", "1", "true", "null"] {
            let response = data_get(&parse_body(raw), "host_label").expect_err("500s");
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }
        let data = parse_body(r#"{"host_label": "h"}"#);
        assert!(data_get(&data, "host_label").expect("found").is_some());
        assert!(data_get(&data, "missing").expect("none").is_none());
    }

    /// `(value or "").strip()`: falsy → `""`, strings strip (Python
    /// semantics, incl. U+0085 and U+001C-U+001F), truthy non-strings 500.
    #[test]
    fn or_empty_strips_and_500s() {
        assert_eq!(or_empty_lossy(None).expect("missing"), "");
        assert_eq!(
            or_empty_lossy(data_get(&parse_body(r#"{"v": "  x  "}"#), "v").expect("get"))
                .expect("stripped"),
            "x"
        );
        assert_eq!(
            or_empty_lossy(data_get(&parse_body("{\"v\": \" \\u001cx\"}"), "v").expect("get"))
                .expect("py-strip"),
            "x"
        );
        for raw in [
            r#"{"v": ""}"#,
            r#"{"v": null}"#,
            r#"{"v": 0}"#,
            r#"{"v": false}"#,
            r#"{"v": []}"#,
            r#"{"v": {}}"#,
            r#"{}"#,
        ] {
            let data = parse_body(raw);
            assert_eq!(
                or_empty_lossy(data_get(&data, "v").expect("get")).expect("falsy"),
                "",
                "{raw}"
            );
        }
        for raw in [r#"{"v": 123}"#, r#"{"v": true}"#, r#"{"v": [1]}"#] {
            let data = parse_body(raw);
            let response = or_empty_lossy(data_get(&data, "v").expect("get")).expect_err("500s");
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }
        // `py_strip` matches `str.strip()` on the non-`White_Space` gap.
        assert_eq!(py_strip(" \u{1c}x\u{2028}"), "x");
        assert_eq!(py_strip("  x "), "x");
    }

    /// The clean variant additionally 500s on surrogate-carrying strings
    /// (Django's `UnicodeEncodeError` at the Postgres encode).
    #[test]
    fn or_empty_clean_rejects_surrogates() {
        let dirty = parse_body("{\"v\": \"a\\ud800b\"}");
        let response = or_empty_clean(data_get(&dirty, "v").expect("get")).expect_err("500s");
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        // ...while the lossy variant flows through (compared positions).
        assert_eq!(
            or_empty_lossy(data_get(&dirty, "v").expect("get")).expect("lossy"),
            "a\u{fffd}b"
        );
        let clean = parse_body(r#"{"v": "  ok  "}"#);
        assert_eq!(
            or_empty_clean(data_get(&clean, "v").expect("get")).expect("clean"),
            "ok"
        );
    }

    /// Query params: last value wins, empty/missing disables.
    #[test]
    fn query_param_last_wins() {
        use crate::license::OneOrMany;
        let params: crate::license::QueryMap = [
            ("a".to_owned(), OneOrMany::One("1".to_owned())),
            (
                "multi".to_owned(),
                OneOrMany::Many(vec!["x".to_owned(), "y".to_owned()]),
            ),
            ("empty".to_owned(), OneOrMany::One(String::new())),
        ]
        .into_iter()
        .collect();
        assert_eq!(query_param(&params, "a").as_deref(), Some("1"));
        assert_eq!(query_param(&params, "multi").as_deref(), Some("y"));
        assert_eq!(query_param(&params, "empty"), None);
        assert_eq!(query_param(&params, "missing"), None);
    }

    fn test_state() -> AppState {
        AppState::with_edge(
            "0.1.0",
            crate::edge::EdgeHandle::for_tests("http://127.0.0.1:1"),
        )
    }

    /// Empty bodies validate as `{}`; unparsable JSON 400s with the
    /// lowercase `ParseError`; non-JSON proxies to Django (502 here —
    /// the test edge points at a dead upstream — proving the proxy path,
    /// not a parse).
    #[tokio::test]
    async fn read_request_data_envelope() {
        let state = test_state();
        let empty = Request::builder()
            .uri("/")
            .body(axum::body::Body::empty())
            .expect("request");
        assert_eq!(
            read_request_data(&state, empty).await.expect("empty"),
            JVal::Object(JObject::new())
        );

        let bad = Request::builder()
            .uri("/")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_LENGTH, "9")
            .body(axum::body::Body::from("{bad json"))
            .expect("request");
        let response = read_request_data(&state, bad).await.expect_err("400s");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("body");
        let text = String::from_utf8(body.to_vec()).expect("utf8");
        assert!(
            text.starts_with("{\"detail\":\"JSON parse error - "),
            "{text}"
        );

        let form = Request::builder()
            .uri("/")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::CONTENT_LENGTH, "3")
            .body(axum::body::Body::from("a=1"))
            .expect("request");
        let response = read_request_data(&state, form).await.expect_err("proxies");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }
}
