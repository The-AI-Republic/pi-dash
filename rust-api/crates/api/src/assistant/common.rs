#![forbid(unsafe_code)]

//! Shared HTTP shell for the assistant config handlers (D-06, stage 5).
//!
//! Ports the request-edge half of
//! `apps/api/pi_dash/assistant/views/llm_config.py:1-185` and
//! `apps/api/pi_dash/assistant/views/stt_config.py:1-147` that both config
//! surfaces share: session auth (`BaseAPIView.authentication_classes`,
//! `IsAuthenticated`), DRF body parsing (`JSONParser` + `ParseError`),
//! `CharField`/`ChoiceField` input coercion with the exact failure strings
//! (probed against DRF 3.16.1 in the PIDASHCONV-256 run), the
//! `{"field": ["msg"]}` error rendering, the `AssistantError` rendering
//! (`{"error", "detail"}` + `http_status`), the crypto/SSRF wiring, and the
//! config-row SQL. The per-surface differences (field sets, cross-field
//! rules, probe classification) stay in [`super::llm_config`] and
//! [`super::stt_config`].
//!
//! Fixture ids: F-A6-01 (config shapes), F-A6-04 (key encrypt on save),
//! F-A6-05 (base_url SSRF check on save).
//!
//! Translation notes (ported as written):
//!
//! * Malformed JSON answers 400 `{"detail": "JSON parse error - ..."}`.
//!   The detail text after the dash comes from this engine's scanner, not
//!   CPython's — the status and the key are the contract; engines disagree
//!   on the human text (same precedent as the loop handlers).
//! * A non-object JSON body answers 400
//!   `{"non_field_errors": ["Invalid data. Expected a dictionary, but got
//!   <type>."]}` with DRF's type names (`list`, `str`, `int`, `float`,
//!   `bool`, `NoneType`).
//! * Per-field order is coerce → `max_length` → custom `validate_<field>`;
//!   the first failure per field wins and every field is collected in
//!   `Meta.fields` order before the cross-field `validate()` runs.
//! * `cfg.save()` failures (even operational ones) answer 400
//!   `{"error": "invalid"}` (`llm_config.py:72-73`, `stt_config.py:74-76`).
//! * Decrypt failures split like the views' `except AssistantError`: data
//!   problems (`CryptoError::NotConfigured`, i.e. `AssistantNotConfigured`)
//!   render through the caller's shape, operational problems
//!   (`CryptoError::Transport`, i.e. a propagating exception) answer the
//!   generic 500.
//! * The in-process decrypted-key cache (`runtime/llm.py:25-64`) is
//!   deliberately not ported: it is a pure latency optimization with no
//!   observable response difference, and holding shared mutable cache state
//!   would need an `AppState` (foundation) change. Every read decrypts
//!   directly, which is strictly fresher than Python after a key change.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::Value;

use pidash_services::assistant::crypto::{decrypt, encrypt, CryptoConfig, CryptoError};
use pidash_services::assistant::ssrf::{is_blocked, SystemResolver};
use pidash_types::assistant::errors::AssistantError;

use crate::state::AppState;

// ---------------------------------------------------------------------------
// failure rendering
// ---------------------------------------------------------------------------

/// Handler failure with its exact status + body.
#[derive(Debug, PartialEq)]
pub struct Failure {
    pub(crate) status: StatusCode,
    pub(crate) body: String,
}

impl Failure {
    fn new(status: StatusCode, body: String) -> Self {
        Self { status, body }
    }

    /// 401, DRF `NotAuthenticated` (anonymous on a guarded endpoint).
    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            crate::license::UNAUTHENTICATED_BODY.to_owned(),
        )
    }

    /// 500, `handle_exception` generic branch.
    pub fn server_error() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            crate::license::SERVER_ERROR_BODY.to_owned(),
        )
    }

    /// 400, DRF `ParseError` for an unparseable body.
    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            format!("{{\"detail\":{}}}", json_string(&message.into())),
        )
    }

    /// 400, serializer `errors` dict (`{"field": ["msg", ...]}` in field
    /// order; `preserve_order` keeps insertion order on the wire).
    pub fn field_errors(errors: Vec<(String, Vec<String>)>) -> Self {
        let mut map = serde_json::Map::with_capacity(errors.len());
        for (field, messages) in errors {
            map.insert(
                field,
                Value::Array(messages.into_iter().map(Value::String).collect()),
            );
        }
        Self::new(
            StatusCode::BAD_REQUEST,
            serde_json::to_string(&Value::Object(map)).expect("error body serializes"),
        )
    }

    /// 415, DRF's `UnsupportedMediaType` for a non-empty body under an
    /// unhandled content type. The raw header is echoed verbatim (a
    /// missing header arrives as `text/plain`, the WSGI default).
    pub fn unsupported_media_type(raw_content_type: &str) -> Self {
        Self::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            format!(
                "{{\"detail\":{}}}",
                json_string(&format!(
                    "Unsupported media type \"{raw_content_type}\" in request."
                ))
            ),
        )
    }

    /// A bare `{"error": ...}` body with its status (the `save()` failure
    /// branch, `llm_config.py:73`, `stt_config.py:76` — no `detail` key).
    pub fn bare_error(status: StatusCode, error: &str) -> Self {
        Self::new(status, format!("{{\"error\":{}}}", json_string(error)))
    }

    /// A view-inline `{"error", "detail"}` body with its status
    /// (`base_url_blocked`, `description_required`, the 502s, ...).
    pub fn error_body(status: StatusCode, error: &str, detail: &str) -> Self {
        Self::new(
            status,
            format!(
                "{{\"error\":{},\"detail\":{}}}",
                json_string(error),
                json_string(detail)
            ),
        )
    }

    /// An `AssistantError` as the views raise it: `{"error": code,
    /// "detail": detail}` with `exc.http_status` (`errors.py:14-27`).
    pub fn assistant_error(err: &AssistantError) -> Self {
        let status =
            StatusCode::from_u16(err.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        Self::error_body(status, err.code(), err.detail())
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        Response::builder()
            .status(self.status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(self.body))
            .expect("failure response")
    }
}

/// Map a [`CryptoError`] the way the views' `try/except AssistantError`
/// does: data problems render through `shape`, operational problems
/// propagate to the generic 500.
pub fn crypto_failure(
    err: CryptoError,
    shape: impl FnOnce(&AssistantError) -> Response,
) -> Response {
    match err {
        CryptoError::NotConfigured(assistant) => shape(&assistant),
        CryptoError::Transport(_) => Failure::server_error().into_response(),
    }
}

pub fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// 200 `{"ok": true}` (test-endpoint success, `llm_config.py:107`,
/// `stt_config.py:109`).
pub fn ok_true() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(r#"{"ok":true}"#))
        .expect("ok response")
}

/// 200 `{"ok": false, "error_code": code}` (test-endpoint gates and
/// unclassified probe failures, `llm_config.py:91,93,97,102`,
/// `stt_config.py:94,96,100,105`). Key order `ok`, `error_code`.
pub fn ok_false(code: &str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(format!(
            "{{\"ok\":false,\"error_code\":{}}}",
            json_string(code)
        )))
        .expect("ok-false response")
}

/// 200 `{"ok": false, "error_code": code, "detail": detail}`
/// (classified probe failures, `llm_config.py:108`,
/// `stt_config.py:111`). Key order `ok`, `error_code`, `detail`.
pub fn ok_false_detail(code: &str, detail: &str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(format!(
            "{{\"ok\":false,\"error_code\":{},\"detail\":{}}}",
            json_string(code),
            json_string(detail)
        )))
        .expect("ok-false-detail response")
}

// ---------------------------------------------------------------------------
// request plumbing
// ---------------------------------------------------------------------------

pub fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Failure> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or_else(Failure::server_error)
}

/// `request.user` or the 401. Hash-verified DRF `SessionAuthentication`
/// via the shared license plumbing (read-only): bad session,
/// unknown/inactive user, or hash mismatch is anonymous.
pub async fn actor(
    state: &AppState,
    pool: &sqlx::PgPool,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<crate::license::Actor, Failure> {
    match crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
    {
        Ok(Some(actor)) => Ok(actor),
        Ok(None) => Err(Failure::unauthorized()),
        Err(_) => Err(Failure::server_error()),
    }
}

/// One parsed body field: a JSON/form value, or an uploaded file (its
/// name is all the views ever observe — `str(UploadedFile)` is the
/// filename, verified live in the PIDASHCONV-256 run).
#[derive(Debug, Clone, PartialEq)]
pub enum BodyField {
    Json(Value),
    File { filename: String },
}

/// An object-like field map: insertion-ordered pairs (`serde_json::Map`
/// only serves `Value`, so repeated form keys resolve last-wins through
/// [`body_get`], matching Django's `QueryDict.get`).
pub type BodyMap = Vec<(String, BodyField)>;

/// Last value for `key`, or `None` when absent (Django's
/// `QueryDict.get` returns the last value).
pub fn body_get<'a>(body: &'a BodyMap, key: &str) -> Option<&'a BodyField> {
    body.iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, field)| field)
}

fn body_insert(body: &mut BodyMap, key: String, field: BodyField) {
    if let Some(slot) = body.iter_mut().find(|(name, _)| *name == key) {
        slot.1 = field;
    } else {
        body.push((key, field));
    }
}

/// A parsed PUT/POST body: an object-like field map, a JSON scalar, or
/// JSON null (which has its own `non_field_errors` message).
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedBody {
    Object(BodyMap),
    Scalar(Value),
    Null,
}

/// Parse a PUT/POST body with DRF's content dispatch (probed live
/// against Django in the PIDASHCONV-256 run):
///
/// * An empty body is `{}` under **any** content type (the parsers never
///   run) — not a `ParseError`.
/// * `application/json` (essence match: case-insensitive, parameters
///   ignored) parses JSON; failure is DRF's `ParseError` 400. JSON
///   `null` is `{"non_field_errors": ["No data provided"]}`, any other
///   scalar the `Expected a dictionary, but got <type>.` shape.
/// * `application/x-www-form-urlencoded` parses like Django's
///   `QueryDict` (percent-decoding, `+` → space, last value wins).
/// * `multipart/form-data` parses text fields the same way; file parts
///   become [`BodyField::File`]. A missing boundary is DRF's exact
///   `Multipart form parse error - Invalid boundary in multipart: None`
///   400; an otherwise unparseable body degrades to `{}` like Django.
/// * Anything else (including a missing header, which the WSGI server
///   reports as `text/plain`) is 415 with the raw header echoed
///   verbatim: `Unsupported media type "<ct>" in request.`
pub fn parse_body(body: &[u8], content_type: Option<&str>) -> Result<ParsedBody, Failure> {
    if body.is_empty() {
        return Ok(ParsedBody::Object(Vec::new()));
    }
    let raw = content_type.unwrap_or("text/plain");
    let essence = raw.split(';').next().unwrap_or("").trim().to_lowercase();
    match essence.as_str() {
        "application/json" => match serde_json::from_slice(body) {
            Ok(Value::Object(map)) => Ok(ParsedBody::Object(
                map.into_iter()
                    .map(|(key, value)| (key, BodyField::Json(value)))
                    .collect::<BodyMap>(),
            )),
            Ok(Value::Null) => Ok(ParsedBody::Null),
            Ok(other) => Ok(ParsedBody::Scalar(other)),
            Err(err) => Err(Failure::parse_error(format!("JSON parse error - {err}"))),
        },
        "application/x-www-form-urlencoded" => Ok(ParsedBody::Object(parse_urlencoded(body))),
        "multipart/form-data" => parse_multipart(body, raw),
        _ => Err(Failure::unsupported_media_type(raw)),
    }
}

/// The request's content type, if it is valid header text.
pub fn request_content_type(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
}

/// Parse a PUT body for the config endpoints: the object map, or the
/// `non_field_errors` 400 (scalars and null included).
pub fn parse_put_body(body: &[u8], content_type: Option<&str>) -> Result<BodyMap, Failure> {
    match parse_body(body, content_type) {
        Ok(ParsedBody::Object(data)) => Ok(data),
        Ok(ParsedBody::Scalar(value)) => Err(non_field_failure(&value)),
        Ok(ParsedBody::Null) => Err(non_field_failure(&Value::Null)),
        Err(failure) => Err(failure),
    }
}

/// Render the `non_field_errors` 400 for a JSON scalar body: `null` is
/// `No data provided`, anything else names its DRF type.
pub fn non_field_failure(value: &Value) -> Failure {
    let message = match value {
        Value::Null => "No data provided".to_owned(),
        other => format!(
            "Invalid data. Expected a dictionary, but got {}.",
            json_type_name(other)
        ),
    };
    Failure::field_errors(vec![("non_field_errors".to_owned(), vec![message])])
}

/// Django `QueryDict` over a urlencoded body: `&`-separated, `name` or
/// `name=value`, `+` → space, forgiving `%XX`, last value wins.
fn parse_urlencoded(body: &[u8]) -> BodyMap {
    let mut map: BodyMap = Vec::new();
    for pair in body.split(|b| *b == b'&') {
        if pair.is_empty() {
            continue;
        }
        let (name, value) = match pair.iter().position(|b| *b == b'=') {
            Some(i) => (&pair[..i], &pair[i + 1..]),
            None => (pair, &[][..]),
        };
        body_insert(
            &mut map,
            urldecode(name),
            BodyField::Json(Value::String(urldecode(value))),
        );
    }
    map
}

/// Forgiving percent-decoding (`+` → space, malformed `%` passes
/// through, UTF-8 with replacement — Django's `unquote_plus`).
fn urldecode(raw: &[u8]) -> String {
    let mut out: Vec<u8> = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < raw.len() => {
                let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
                match (hex(raw[i + 1]), hex(raw[i + 2])) {
                    (Some(hi), Some(lo)) => {
                        out.push(hi << 4 | lo);
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

/// Lenient multipart parsing for the config endpoints: text fields
/// decode (lossy) with last-wins; file parts become [`BodyField::File`];
/// segments without a usable field name are skipped (Django degrades
/// the same bodies to `{}`, verified live).
fn parse_multipart(body: &[u8], raw_content_type: &str) -> Result<ParsedBody, Failure> {
    let boundary = multipart_boundary(raw_content_type).ok_or_else(|| {
        Failure::parse_error("Multipart form parse error - Invalid boundary in multipart: None")
    })?;
    let delimiter = format!("--{boundary}");
    let positions: Vec<usize> = {
        let mut positions = Vec::new();
        let (mut start, len) = (0, delimiter.len());
        while let Some(found) = body[start..]
            .windows(len)
            .position(|window| window == delimiter.as_bytes())
        {
            positions.push(start + found);
            start += found + len;
        }
        positions
    };
    let mut map: BodyMap = Vec::new();
    let mut spans: Vec<&[u8]> = positions
        .windows(2)
        .map(|pair| &body[pair[0] + delimiter.len()..pair[1]])
        .collect();
    if let Some(last) = positions.last() {
        spans.push(&body[last + delimiter.len()..]);
    }
    for segment in spans {
        // The closing `--boundary--` tail and any preamble-adjacent
        // fragment start with `--` after stripping.
        let segment = strip_segment(segment);
        if segment.starts_with(b"--") {
            continue;
        }
        let Some((name, file, value)) = multipart_part(segment) else {
            continue;
        };
        body_insert(
            &mut map,
            name,
            match file {
                Some(filename) => BodyField::File { filename },
                None => BodyField::Json(Value::String(value)),
            },
        );
    }
    Ok(ParsedBody::Object(map))
}

/// The `boundary=` parameter of the content type (quotes stripped).
/// `None` is missing or empty, matching the probed `...: None` message.
fn multipart_boundary(raw_content_type: &str) -> Option<String> {
    for param in raw_content_type.split(';').skip(1) {
        let param = param.trim();
        if let Some(value) = param
            .strip_prefix("boundary=")
            .or_else(|| param.strip_prefix("BOUNDARY="))
        {
            let value = value.trim().trim_matches('"').to_owned();
            if value.is_empty() {
                return None;
            }
            return Some(value);
        }
    }
    None
}

/// Strip the single CRLF pair framing a segment: the delimiter's
/// trailing CRLF up front and the next delimiter's leading CRLF at the
/// back. Content that ends in its own CRLF keeps it (only one layer is
/// removed), matching Django's framing split.
fn strip_segment(segment: &[u8]) -> &[u8] {
    let segment = segment.strip_prefix(b"\r\n").unwrap_or(segment);
    segment.strip_suffix(b"\r\n").unwrap_or(segment)
}

/// Split one framed segment into `(name, filename?, text)`.
/// Returns `None` when the segment has no usable field name.
fn multipart_part(segment: &[u8]) -> Option<(String, Option<String>, String)> {
    let split = segment
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?;
    let (header_bytes, value_bytes) = segment.split_at(split);
    let headers = String::from_utf8_lossy(header_bytes);
    let disposition = headers
        .lines()
        .find_map(|line| line.strip_prefix("Content-Disposition:"))?;
    if !disposition.trim_start().starts_with("form-data") {
        return None;
    }
    let name = disposition_param(disposition, "name")?;
    let filename = disposition_param(disposition, "filename");
    let text = String::from_utf8_lossy(&value_bytes[4..]).into_owned();
    Some((name, filename, text))
}

/// `name="value"` (or bare `name=value`) from a disposition header.
fn disposition_param(disposition: &str, key: &str) -> Option<String> {
    for part in disposition.split(';').skip(1) {
        let part = part.trim();
        if let Some(value) = part.strip_prefix(&format!("{key}=")) {
            let value = value.trim();
            if let Some(unquoted) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                return Some(unquoted.to_owned());
            }
            return Some(value.to_owned());
        }
    }
    None
}

/// DRF's type names for the `non_field_errors` message
/// (`type(data).__name__` over JSON values).
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "int"
            } else {
                "float"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

// ---------------------------------------------------------------------------
// DRF field coercion (probed against DRF 3.16.1)
// ---------------------------------------------------------------------------

/// DRF `CharField.to_internal_value` coercion (`fields.py:759-767`):
/// `None` is `This field may not be null.`, bools/lists/dicts are `Not a
/// valid string.`, numbers stringify (`str(data)`), strings pass through.
/// Callers then run [`run_char_field`], which owns the strip, the
/// validators, and the custom `validate_<field>`.
pub fn coerce_string(value: &Value) -> Result<String, &'static str> {
    match value {
        Value::Null => Err("This field may not be null."),
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => Err("Not a valid string."),
        Value::Number(n) => Ok(n.to_string()),
        Value::String(s) => Ok(s.clone()),
    }
}

/// Per-field CharField rules: the column `max_length` and whether the
/// Django `URLValidator` applies (`base_url` is a model `URLField`;
/// `model_name`/`api_key` are plain `CharField`s).
#[derive(Debug, Clone, Copy)]
pub struct CharFieldRules {
    pub max_length: usize,
    pub url: bool,
}

/// DRF `CharField.run_validation` + `run_validators` + the custom
/// `validate_<field>` (`fields.py:749-757`, `serializers.py`):
///
/// 1. A value that is empty or whitespace-only short-circuits to `""`
///    (all four config fields allow blank) — the `max_length` / null /
///    URL validators are skipped, but the custom `validate_<field>`
///    still runs on `""` (verified live against the real serializers in
///    the PIDASHCONV-256 run: 600 spaces for `model_name` fail with the
///    model-name message, not `max_length`).
/// 2. Otherwise the value is stripped (`trim_whitespace`, always on
///    here), then `MaxLengthValidator`, Django's
///    `ProhibitNullCharactersValidator`, the `URLValidator` for URL
///    fields, and finally the custom validator run in that order — the
///    first failure wins.
/// 3. The custom validator receives the stripped value and its output is
///    what the view stores (so padded input stores stripped).
///
/// `ProhibitSurrogateCharactersValidator` is deliberately absent: JSON
/// strings are valid UTF-8 (Rust `String` cannot hold surrogates) and
/// `serde_json` rejects lone `\ud800` escapes at parse time, so the
/// validator is unreachable on this path.
pub fn run_char_field(
    value: &Value,
    rules: &CharFieldRules,
    custom: impl FnOnce(&str) -> Result<String, &'static str>,
) -> Result<String, String> {
    let coerced = coerce_string(value).map_err(|message| message.to_owned())?;
    if coerced.trim().is_empty() {
        return custom("").map_err(|message| message.to_owned());
    }
    let stripped = coerced.trim().to_owned();
    check_max_length(&stripped, rules.max_length)?;
    if stripped.contains('\0') {
        return Err("Null characters are not allowed.".to_owned());
    }
    if rules.url {
        validate_django_url(&stripped)?;
    }
    custom(&stripped).map_err(|message| message.to_owned())
}

/// `CharField(max_length=...)`: Python `len()` counts code points, so the
/// cap is in `chars()`, not bytes (semantic trap).
pub fn check_max_length(value: &str, max: usize) -> Result<(), String> {
    if value.chars().count() > max {
        return Err(format!(
            "Ensure this field has no more than {max} characters."
        ));
    }
    Ok(())
}

/// Python `str()` over a JSON value for `ChoiceField` failure messages:
/// scalars via [`py_scalar_display`], containers via [`py_repr`].
fn py_display(value: &Value) -> String {
    py_scalar_display(value)
}

/// Python `repr()` over a JSON container for failure/coercion messages
/// (`str([1])` → `"[1]"`, `str({"a": 1})` → `"{'a': 1}"`): strings render
/// single-quoted, other scalars via [`py_scalar_display`].
pub fn py_repr(value: &Value) -> String {
    match value {
        Value::String(s) => format!("'{s}'"),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("'{k}': {}", py_repr(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        _ => py_scalar_display(value),
    }
}

/// Python `str()` over a JSON scalar: `True`/`False`/`None`, numbers
/// as-is, strings verbatim.
pub fn py_scalar_display(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => py_repr(value),
    }
}

/// Django `URLValidator` (`django/core/validators.py:69-160`) as DRF's
/// `URLField` runs it: `max_length` first (handled by the caller), then
/// this. Every failure reports `Enter a valid URL.` — the verdicts below
/// were pinned against the live validator in the PIDASHCONV-256 run
/// (see the `django_url_truth_table` test).
///
/// Structure: length cap (chars, unreachable under the 500 column cap but
/// kept for fidelity), tab/CR/LF rejection, scheme allowlist
/// (`http/https/ftp/ftps`, first `://` split, lowercased), authority
/// decomposition (userinfo, bracketed IPv6, port), host classification
/// (IPv4 octets without leading zeros, `localhost`, or dot-labels with
/// the TLD rule), an IDN second pass over the ACE form, the 1–5-digit
/// port rule, the no-whitespace path rule, and the 253-char hostname cap.
pub fn validate_django_url(value: &str) -> Result<(), String> {
    const INVALID: &str = "Enter a valid URL.";
    let invalid = || Err(INVALID.to_owned());
    if value.chars().count() > 2048 {
        return invalid();
    }
    if value.contains(['\t', '\r', '\n']) {
        return invalid();
    }
    let scheme = value.split("://").next().unwrap_or("").to_lowercase();
    if !["http", "https", "ftp", "ftps"].contains(&scheme.as_str()) {
        return invalid();
    }
    let after = match value.split_once("://") {
        Some((_, rest)) => rest,
        None => return invalid(),
    };
    let auth_end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let (authority, rest) = after.split_at(auth_end);
    if rest.chars().any(|c| c.is_whitespace()) {
        return invalid();
    }
    let hostport = match authority.rfind('@') {
        Some(i) => {
            if !valid_url_userinfo(&authority[..i]) {
                return invalid();
            }
            &authority[i + 1..]
        }
        None => authority,
    };
    let (host, port, bracketed) = match split_url_host_port(hostport) {
        Some(triple) => triple,
        None => return invalid(),
    };
    // A bracketed literal already passed the strict IPv6 check; only
    // bare hosts go through the localhost/IPv4/label classification
    // (plus the IDN second pass).
    if !bracketed && !valid_url_host(host) && !valid_url_host_idn(host) {
        return invalid();
    }
    if let Some(port) = port {
        if port.is_empty() || port.len() > 5 || !port.bytes().all(|b| b.is_ascii_digit()) {
            return invalid();
        }
    }
    let hostname = host.to_lowercase();
    if hostname.is_empty() || hostname.chars().count() > 253 {
        return invalid();
    }
    Ok(())
}

/// The `user:pass` part of the authority
/// (`(?:[^\s:@/]+(?::[^\s:@/]*)?@)?`): no whitespace, colon, `@`, or
/// slash in either half; at most one colon.
fn valid_url_userinfo(userinfo: &str) -> bool {
    if userinfo.is_empty() {
        return false;
    }
    let (name, pass) = match userinfo.split_once(':') {
        Some((name, pass)) => (name, Some(pass)),
        None => (userinfo, None),
    };
    let clean = |part: &str| {
        !part.is_empty()
            && !part
                .chars()
                .any(|c| c.is_whitespace() || c == ':' || c == '@' || c == '/')
    };
    if !clean(name) {
        return false;
    }
    if let Some(pass) = pass {
        // An empty password (`u:@host`) matches `(?::[^\s:@/]*)?`.
        if !pass
            .chars()
            .all(|c| !c.is_whitespace() && c != ':' && c != '@' && c != '/')
        {
            return false;
        }
    }
    true
}

/// Split the post-userinfo authority into host and optional port:
/// bracketed IPv6 (`[::1]`, strict address, optional `:port`) or a bare
/// host with at most one colon introducing the port. The flag reports a
/// validated bracketed literal, which skips label classification.
fn split_url_host_port(hostport: &str) -> Option<(&str, Option<&str>, bool)> {
    if let Some(rest) = hostport.strip_prefix('[') {
        let end = rest.find(']')?;
        let (inside, after) = rest.split_at(end);
        if !valid_ipv6_literal(inside) {
            return None;
        }
        let after = &after[1..];
        if after.is_empty() {
            return Some((&hostport[1..end + 1], None, true));
        }
        let port = after.strip_prefix(':')?;
        return Some((&hostport[1..end + 1], Some(port), true));
    }
    match hostport.split_once(':') {
        None => Some((hostport, None, false)),
        Some((host, port)) => {
            if host.contains(':') {
                return None;
            }
            Some((host, Some(port), false))
        }
    }
}

/// The bracket validator: `[0-9a-f:.]+` (case-insensitive) plus a strict
/// IPv6 parse (`validate_ipv6_address`, i.e. `inet_pton`-equivalent).
fn valid_ipv6_literal(inside: &str) -> bool {
    if inside.is_empty()
        || !inside
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '.' || c == ':')
    {
        return false;
    }
    inside.parse::<std::net::Ipv6Addr>().is_ok()
}

/// Host classification over the raw host: `localhost`, IPv4, or
/// dot-labels with the TLD rule (case-insensitive ASCII; `\u00a1-\uffff`
/// letters allowed like the Django regex).
fn valid_url_host(host: &str) -> bool {
    let host = host.to_lowercase();
    if host == "localhost" {
        return true;
    }
    if is_django_ipv4(&host) {
        return true;
    }
    valid_domain_labels(&host)
}

/// IDN second pass (`validators.py:131-142`): punycode the host to ACE
/// and re-run the classification. Only reachable for non-ASCII hosts
/// the first pass rejects.
fn valid_url_host_idn(host: &str) -> bool {
    if host.is_ascii() {
        return false;
    }
    match idna::domain_to_ascii(host) {
        Ok(ace) => valid_url_host(&ace),
        Err(_) => false,
    }
}

/// Django `ipv4_re`: four dot-parts, each `0`, `25[0-5]`, `2[0-4][0-9]`,
/// `1[0-9]{1,2}`, or `[1-9][0-9]?` — i.e. 0–255 with no leading zeros.
fn is_django_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|part| {
        !part.is_empty()
            && part.bytes().all(|b| b.is_ascii_digit())
            && !(part.len() > 1 && part.starts_with('0'))
            && part.parse::<u16>().is_ok_and(|n| n <= 255)
    })
}

/// Dot-labels (`hostname_re domain_re tld_re`): one trailing dot is
/// tolerated; middle labels are 1–63 chars with alnum ends; the TLD is
/// 2–63 chars of letters/hyphens or an `xn--` punycode label, never
/// starting or ending with a hyphen.
fn valid_domain_labels(host: &str) -> bool {
    let trimmed = host.strip_suffix('.').unwrap_or(host);
    let labels: Vec<&str> = trimmed.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let (middle, tld) = labels.split_at(labels.len() - 1);
    for label in middle {
        if !valid_domain_label(label) {
            return false;
        }
    }
    valid_tld_label(tld[0])
}

fn url_label_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || ('\u{a1}'..='\u{ffff}').contains(&c)
}

fn valid_domain_label(label: &str) -> bool {
    let chars: Vec<char> = label.chars().collect();
    if chars.is_empty() || chars.len() > 63 {
        return false;
    }
    if !url_label_char(chars[0]) || !url_label_char(chars[chars.len() - 1]) {
        return false;
    }
    chars.iter().all(|c| url_label_char(*c) || *c == '-')
}

fn valid_tld_label(tld: &str) -> bool {
    if tld.starts_with('-') || tld.ends_with('-') {
        return false;
    }
    if let Some(rest) = tld.strip_prefix("xn--") {
        return !rest.is_empty()
            && rest.len() <= 59
            && rest.bytes().all(|b| b.is_ascii_alphanumeric());
    }
    let chars: Vec<char> = tld.chars().collect();
    if chars.len() < 2 || chars.len() > 63 {
        return false;
    }
    chars
        .iter()
        .all(|c| c.is_ascii_alphabetic() || ('\u{a1}'..='\u{ffff}').contains(c) || *c == '-')
}

/// DRF `ChoiceField`: `None` is `may not be null`; anything else must
/// equal a choice exactly (case-sensitive, no trimming), else
/// `"<display>" is not a valid choice.` with the Python-`str()` display.
pub fn validate_choice(value: &Value, choices: &[&str]) -> Result<String, String> {
    if value.is_null() {
        return Err("This field may not be null.".to_owned());
    }
    let display = py_display(value);
    if choices.contains(&display.as_str()) {
        Ok(display)
    } else {
        Err(format!("\"{display}\" is not a valid choice."))
    }
}

// ---------------------------------------------------------------------------
// crypto + SSRF wiring
// ---------------------------------------------------------------------------

/// Backend settings from the request's `Settings` (the `GitLabConfig`
/// precedent: settings never reach library code). `AWS_REGION` has no
/// `Settings` field, so it stays an env read like
/// [`CryptoConfig::from_env`]; everything else comes from the resolved
/// settings so both backends read the same effective config as Django.
pub fn crypto_config(state: &AppState) -> CryptoConfig {
    let assistant = &state.settings().assistant;
    CryptoConfig {
        backend: assistant.crypto_backend.clone(),
        fernet_keys: assistant.encryption_key.clone(),
        kms_key_id: assistant.kms_key_id.clone(),
        aws_region: std::env::var("AWS_REGION").unwrap_or_default(),
        kms_endpoint_url: assistant.kms_endpoint_url.clone(),
    }
}

/// `crypto.encrypt(api_key)` (`llm_config.py:67`, `stt_config.py:70`).
pub async fn encrypt_api_key(
    config: &CryptoConfig,
    plaintext: &str,
) -> Result<Vec<u8>, CryptoError> {
    let transport = super::kms::HttpKmsTransport::from_env(config);
    encrypt(config, plaintext, &transport)
}

/// `crypto.decrypt(cfg.api_key_encrypted)` (the test endpoints).
pub async fn decrypt_api_key(config: &CryptoConfig, token: &[u8]) -> Result<String, CryptoError> {
    let transport = super::kms::HttpKmsTransport::from_env(config);
    decrypt(config, token, &transport)
}

/// Save-time SSRF guard (`if base_url and ssrf.is_blocked(base_url)`,
/// `llm_config.py:53`, `stt_config.py:56`): consulted only when
/// `base_url` is non-empty, with the cloud flag from settings and live
/// libc resolution (`ssrf.py:29-43`).
pub fn base_url_blocked(state: &AppState, base_url: &str) -> bool {
    if base_url.is_empty() {
        return false;
    }
    is_blocked(
        base_url,
        state.settings().assistant.block_private_urls,
        &SystemResolver,
    )
}

/// `last_verified_at.isoformat()` (`llm_config.py:37`,
/// `stt_config.py:40`): UTC renders `+00:00`; microseconds appear only
/// when nonzero (chrono `AutoSi` matches both halves).
pub fn isoformat(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false)
}

/// `workspace_role_by_slug` (`core/permissions.py:48-56`): the caller's
/// active `WorkspaceMember.role` for the slug workspace, or `None`.
/// `PositiveSmallIntegerField` decodes as `i16`.
pub async fn workspace_role(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
    slug: &str,
) -> Result<Option<i32>, Failure> {
    let row: Option<(i16,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2
             AND wm.is_active AND wm.deleted_at IS NULL
           LIMIT 1"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Failure::server_error())?;
    Ok(row.map(|row| i32::from(row.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coerce_string_matches_drf() {
        assert_eq!(coerce_string(&Value::Number(5.into())), Ok("5".to_owned()));
        assert_eq!(
            coerce_string(&Value::Bool(true)),
            Err("Not a valid string.")
        );
        assert_eq!(
            coerce_string(&Value::Null),
            Err("This field may not be null.")
        );
        assert_eq!(
            coerce_string(&Value::String("m".to_owned())),
            Ok("m".to_owned())
        );
    }

    #[test]
    fn max_length_counts_code_points() {
        assert!(check_max_length("éé", 2).is_ok());
        assert!(check_max_length("éé", 1).is_err());
    }

    #[test]
    fn choice_failures_match_drf() {
        let choices = &["openai_compatible", "anthropic"];
        assert_eq!(
            validate_choice(&Value::String(String::new()), choices),
            Err("\"\" is not a valid choice.".to_owned())
        );
        assert_eq!(
            validate_choice(&Value::Bool(true), choices),
            Err("\"True\" is not a valid choice.".to_owned())
        );
        assert_eq!(
            validate_choice(&Value::Number(5.into()), choices),
            Err("\"5\" is not a valid choice.".to_owned())
        );
        assert_eq!(
            validate_choice(&Value::Null, choices),
            Err("This field may not be null.".to_owned())
        );
        assert_eq!(
            validate_choice(&Value::String("anthropic".to_owned()), choices),
            Ok("anthropic".to_owned())
        );
    }

    #[test]
    fn body_dispatch_matches_drf() {
        // Empty is `{}` under any content type — never a parse error.
        for ct in [
            None,
            Some("application/json"),
            Some("text/plain"),
            Some("multipart/form-data; boundary=x"),
        ] {
            assert_eq!(parse_body(b"", ct), Ok(ParsedBody::Object(Vec::new())));
        }
        // Scalars name their DRF types; null has its own message.
        let json = Some("application/json");
        assert!(matches!(
            parse_body(b"[1]", json),
            Ok(ParsedBody::Scalar(Value::Array(_)))
        ));
        assert_eq!(parse_body(b"5", json), Ok(ParsedBody::Scalar(5.into())));
        assert_eq!(parse_body(b"null", json), Ok(ParsedBody::Null));
        assert_eq!(
            non_field_failure(&Value::Null).body,
            r#"{"non_field_errors":["No data provided"]}"#.to_owned()
        );
        // Malformed JSON is a parse error; unknown types are 415 with the
        // raw header echoed (missing arrives as text/plain).
        assert!(parse_body(b"{bad", json).is_err());
        let err = parse_body(b"a=1", None).expect_err("missing ct is 415");
        assert_eq!(
            err.body,
            r#"{"detail":"Unsupported media type \"text/plain\" in request."}"#.to_owned()
        );
        // Forms parse last-wins with forgiving decoding.
        let form = Some("application/x-www-form-urlencoded");
        assert_eq!(
            parse_body(b"model_name=a&model_name=b+c", form),
            Ok(ParsedBody::Object(vec![(
                "model_name".to_owned(),
                BodyField::Json("b c".into())
            )]))
        );
    }

    #[test]
    fn django_url_truth_table() {
        // Verdicts read off the live Django `URLValidator` in the
        // PIDASHCONV-256 run (not re-derived here).
        let ok = [
            "https://8.8.8.8/v1",
            "https://u:p@8.8.8.8/v1",
            "HTTPS://8.8.8.8/v1",
            "http://localhost:8000/x",
            "http://[::1]/v1",
            "http://u:p@[::1]:8080/x",
            "http://x.com./v1",
            "http://0.0.0.0/v1",
            "http://münchen.de/v1",
            "http://例子.测试/v1",
            "http://xn--mnchen-3ya.de/v1",
            "http://[2001:db8::1]/v1",
            "http://x.com:88888/v1",
            "ftp://8.8.8.8/v1",
            "http://X.COM/v1",
            "HtTp://x.com:80/a?b=c#d",
        ];
        for url in ok {
            assert!(validate_django_url(url).is_ok(), "{url}");
        }
        let bad = [
            "ftp://x/v1",
            "gopher://x.com/",
            "http://myhost/v1",
            "http://x.com:123456/v1",
            "http://x.com:/v1",
            "http://[::1",
            "http://[::zz]/v1",
            "http://-x.com/v1",
            "http://x-.com/v1",
            "http://1.2.3.256/v1",
            "http://01.2.3.4/v1",
            "8.8.8.8/v1",
            "",
            "http://x.com/a b/v1",
            "http://x.com/\u{a0}",
            "http://a..com/v1",
            "http://x.c/v1",
            "http://u ser@x.com/v1",
            "http://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.com/v1",
        ];
        for url in bad {
            assert_eq!(
                validate_django_url(url),
                Err("Enter a valid URL.".to_owned()),
                "{url}"
            );
        }
    }

    #[test]
    fn char_field_pipeline_matches_drf_order() {
        let rules = CharFieldRules {
            max_length: 8,
            url: false,
        };
        // Whitespace-only short-circuits the length check: the custom
        // validator still runs on "".
        assert_eq!(
            run_char_field(&Value::String("   ".to_owned()), &rules, |_| Err("custom")),
            Err("custom".to_owned())
        );
        assert_eq!(
            run_char_field(&Value::String("        ".to_owned()), &rules, |v| Ok(
                v.to_owned()
            )),
            Ok(String::new())
        );
        // Otherwise strip-then-length: padding does not count.
        assert_eq!(
            run_char_field(&Value::String("  ab  ".to_owned()), &rules, |v| Ok(
                v.to_owned()
            )),
            Ok("ab".to_owned())
        );
        // Null bytes fail before the custom validator.
        assert_eq!(
            run_char_field(&Value::String("a\x00b".to_owned()), &rules, |v| Ok(
                v.to_owned()
            )),
            Err("Null characters are not allowed.".to_owned())
        );
    }

    #[test]
    fn isoformat_matches_python() {
        let dt = chrono::DateTime::parse_from_rfc3339("2026-09-29T13:50:51.816714+00:00")
            .expect("parses")
            .with_timezone(&chrono::Utc);
        assert_eq!(isoformat(&dt), "2026-09-29T13:50:51.816714+00:00");
        let whole = chrono::DateTime::parse_from_rfc3339("2026-09-29T13:50:51+00:00")
            .expect("parses")
            .with_timezone(&chrono::Utc);
        assert_eq!(isoformat(&whole), "2026-09-29T13:50:51+00:00");
    }
}
