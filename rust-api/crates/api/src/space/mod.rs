//! Space public-API handlers (D-02, stage 4).
//!
//! Ports `apps/api/pi_dash/space/views/` onto the merged D-02 foundation.
//! Sibling handler issues own their files and share this module's plumbing:
//!
//! * [`intake`] — `views/intake.py` (`IntakeIssuePublicViewSet`: list,
//!   create, retrieve, partial_update, destroy; PIDASHCONV-177).
//! * [`project_meta`] — `views/project.py` + `meta.py` + taxonomy
//!   (PIDASHCONV-174): query builders from
//!   [`project_meta`](pidash_services::space::queries::project_meta),
//!   anchor/error mapping from
//!   [`guards`](pidash_services::space::guards), and the project-lite leaf
//!   from [`lite`](pidash_services::space::serializers::lite).
//! * [`issues`] — `views/issue.py` list/retrieve (PIDASHCONV-175).
//! * [`social`] — `views/issue.py` comments, issue reactions, comment
//!   reactions, votes (PIDASHCONV-176).
//! * [`assets`] — `views/asset.py` S3 assets (PIDASHCONV-178).
//!
//! [`routes`] merges the owned route groups; every other method on the
//! owned paths proxies to Django through the edge fallback (its
//! 405-after-auth and metadata responses live there).
//!
//! Shared plumbing (mirrors the D-01 `license` and D-26 `app_issues`
//! shapes):
//!
//! - [`QueryMap`] / [`query_last`]: Django `QueryDict.get` (last value wins).
//! - [`Denial`]: exact error bodies (`views/base.py:65-103`,
//!   DRF `NotAuthenticated` default).
//! - [`python_dumps`]: `json.dumps` with CPython defaults (`, `/`: `
//!   separators, `ensure_ascii`), for Celery `requested_data` payloads.
//! - [`owned`]: cutover granularity — owned methods serve from Rust, the
//!   rest proxy so Django's 405-after-auth responses survive byte for byte.
//!
//! Sibling handler issues extend [`routes`] with their own routers; merges
//! keep both sides.

use std::collections::HashMap;

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;

use crate::state::AppState;

pub mod assets;
pub mod filters;
pub mod intake;
pub mod issues;
pub mod project_meta;
pub mod sanitize;
pub mod social;

/// Exact bytes of the DRF `NotAuthenticated` denial: anonymous on a guarded
/// route (`request.successful_authenticator` is `None`).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `views/base.py:86-90` (`ObjectDoesNotExist` branch).
pub const NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// `views/base.py:99-103` (generic branch).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// One query value, repeated or not. Mirrors the D-26 `app_issues` shape:
/// `serde_html_form` does not coerce a lone `?key=value` into a sequence,
/// so callers read last-wins like Django's `QueryDict`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

/// The multi-value query map every list handler extracts.
pub type QueryMap = HashMap<String, OneOrMany>;

/// All values for `key`, in order; `None` when absent.
pub fn query_values(query: &QueryMap, key: &str) -> Option<Vec<String>> {
    query.get(key).map(|value| match value {
        OneOrMany::One(one) => vec![one.clone()],
        OneOrMany::Many(many) => many.clone(),
    })
}

/// Django `QueryDict.get`: the last value, or `None`.
pub fn query_last(query: &QueryMap, key: &str) -> Option<String> {
    query_values(query, key).and_then(|values| values.into_iter().last())
}

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated` (anonymous on a guarded route).
    Unauthorized,
    /// 404, `ObjectDoesNotExist` branch.
    NotFound,
    /// 400, `{"error": ...}` (view-inline).
    BadError(String),
    /// 400, pre-rendered serializer-errors body (`{"field": [...]}`).
    BadJson(serde_json::Value),
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::BadJson(body) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(body).expect("serializable denial"),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("static denial response")
    }
}

pub fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Render a guards-layer [`pidash_services::space::guards::ErrorBody`]
/// as the exact wire response.
pub fn guards_error(error: pidash_services::space::guards::ErrorBody) -> Response {
    let status = StatusCode::from_u16(error.status).expect("guard status is a valid code");
    let body = serde_json::to_string(&error.body).expect("serializable guard body");
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("guard error response")
}

/// Render a value body as compact JSON (`JSONRenderer`, `COMPACT_JSON`):
/// no spaces, insertion order preserved (`preserve_order`).
pub fn json_response<T: serde::Serialize>(value: &T) -> Response {
    let body = serde_json::to_string(value).expect("serializable response");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("json response")
}

/// Render an already-serialized compact JSON body (handlers that assemble
/// key order by hand produce the string directly).
pub fn raw_json_response(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("raw json response")
}

/// `json.dumps(value)` with CPython defaults: `separators=(', ', ': ')`,
/// `ensure_ascii=True`, insertion-ordered keys.
///
/// The intake views dump `request.data` (create) and the 3-key update
/// subset (partial_update) into Celery `requested_data` this way
/// (`views/intake.py:157,221`). The input is client-parsed JSON, so only
/// JSON-native types occur; floats render shortest-round-trip (Ryu, same
/// as CPython `repr` for finite values).
pub fn python_dumps(value: &serde_json::Value) -> String {
    let mut out = String::new();
    python_dump_into(&mut out, value);
    out
}

fn python_dump_into(out: &mut String, value: &serde_json::Value) {
    match value {
        serde_json::Value::Null => out.push_str("null"),
        serde_json::Value::Bool(true) => out.push_str("true"),
        serde_json::Value::Bool(false) => out.push_str("false"),
        serde_json::Value::Number(number) => out.push_str(&number.to_string()),
        serde_json::Value::String(text) => python_dump_str(out, text),
        serde_json::Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                python_dump_into(out, item);
            }
            out.push(']');
        }
        serde_json::Value::Object(map) => {
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                python_dump_str(out, key);
                out.push_str(": ");
                python_dump_into(out, item);
            }
            out.push('}');
        }
    }
}

/// CPython `py_encode_basestring_ascii`: `"`/`\` plus the short escapes,
/// every other char outside printable ASCII as `\uXXXX` (astral chars as
/// surrogate pairs). `/`, `<`, `>`, `'` and DEL-adjacent printables pass
/// through exactly like CPython (only `< 0x20` and `> 0x7E` escape).
fn python_dump_str(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 || (ch as u32) == 0x7F => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch if (ch as u32) > 0x7E => {
                let code = ch as u32;
                if code > 0xFFFF {
                    let v = code - 0x10000;
                    out.push_str(&format!(
                        "\\u{:04x}\\u{:04x}",
                        0xD800 + (v >> 10),
                        0xDC00 + (v & 0x3FF)
                    ));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

/// The owned methods on a space path serve from Rust while every other
/// method falls through to Django (its 405-after-auth and metadata
/// responses live there). `HEAD` proxies explicitly: axum would
/// auto-serve it from `get`, but Django defines no `head` and 405s after
/// auth. `OPTIONS` proxies so DRF metadata (401 anon / 200 authed) is
/// preserved.
pub fn owned(
    methods: axum::routing::MethodRouter<AppState>,
    owned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = methods;
    for other in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
        if owned.contains(&other) {
            continue;
        }
        router = match other {
            "GET" => router.get(crate::edge::proxy),
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            "HEAD" => router.head(crate::edge::proxy),
            _ => router.options(crate::edge::proxy),
        };
    }
    router
}

/// Owned D-02 space routes (cutover granularity: registered paths serve
/// from Rust, everything else keeps proxying). Sibling handler issues
/// merge their routers here; merges keep both sides.
pub fn routes() -> Router<AppState> {
    intake::routes()
        .merge(project_meta::routes())
        .merge(issues::routes())
        .merge(social::routes())
        .merge(assets::routes())
}
