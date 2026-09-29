//! User-managed MCP tool-server handlers (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/views/mcp_servers.py:1-165`
//! (`AssistantMCPServerListCreateEndpoint`,
//! `AssistantMCPServerDetailEndpoint`, routes
//! `users/me/ai-assistant/mcp-servers/` and
//! `users/me/ai-assistant/mcp-servers/<uuid:server_id>/` from
//! `assistant/urls.py:100-108`).
//!
//! Per-user CRUD, scoped like the BYOK config endpoints: a user only
//! ever sees and edits their own rows. The auth header is write-only —
//! never echoed back, only its presence (`has_auth_header`).
//!
//! Layering: field validation mirrors
//! `AssistantMCPServerSerializer` (`serializers.py:123-162`, shapes in
//! `pidash_types::assistant::serializers`); the Django `URLField`
//! check is [`django_url_valid`] below; slugs come from
//! `pidash_db::assistant::models::assistant_mcp_server::tool_prefix`,
//! run-unique prefixes and the server cap from
//! `pidash_services::assistant::mcp`, encryption from
//! `pidash_services::assistant::crypto`, the host guard from
//! `pidash_services::assistant::ssrf`. This module owns the HTTP shell
//! (routes, session auth), the validation envelope, the SQL text, and
//! the response rendering.
//!
//! Registration is the cutover granularity (same rule as the `space`,
//! `loop` and `prompting` families): the owned methods serve from Rust
//! while every other method on those paths proxies to Django. `HEAD`
//! rides axum's `get` handling on the list path like Django's
//! `GET`-backed `HEAD`; on the detail path (no GET in Django) `HEAD`
//! proxies so Django's own 405 answers.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Router;
use http_body_util::BodyExt as _;
use serde_json::{Map, Value};

use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::{error_response, json_body, pool_of, request_actor, Denial};

/// `users/me/ai-assistant/mcp-servers/` (under the `api/` include).
pub const SERVERS_PATH: &str = "/api/users/me/ai-assistant/mcp-servers/";
/// `users/me/ai-assistant/mcp-servers/<server_id>/`.
pub const SERVER_DETAIL_PATH: &str = "/api/users/me/ai-assistant/mcp-servers/{server_id}/";

/// Register the owned paths. Every other method proxies to Django (its
/// 405-after-auth and metadata OPTIONS live there).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            SERVERS_PATH,
            axum::routing::get(list_servers)
                .post(create_server)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            SERVER_DETAIL_PATH,
            axum::routing::patch(patch_server)
                .delete(delete_server)
                .get(crate::edge::proxy)
                .head(crate::edge::proxy)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

// ---------------------------------------------------------------------------
// Validation (AssistantMCPServerSerializer, serializers.py:123-162)
// ---------------------------------------------------------------------------

/// DRF default messages used below (stable English strings, verified
/// live against the installed DRF).
const REQUIRED_MSG: &str = "This field is required.";
const NULL_MSG: &str = "This field may not be null.";
const BLANK_MSG: &str = "This field may not be blank.";
const INVALID_STR_MSG: &str = "Not a valid string.";
const INVALID_BOOL_MSG: &str = "Must be a valid boolean.";
const INVALID_URL_MSG: &str = "Enter a valid URL.";
const NULL_CHARS_MSG: &str = "Null characters are not allowed.";
const NAME_REQUIRED_MSG: &str = "A name is required.";
const URL_REQUIRED_MSG: &str = "A server URL is required.";
const URL_HTTP_MSG: &str = "url must be an http(s) URL.";
const URL_NO_CREDENTIALS_MSG: &str = "url must not contain credentials.";

/// Validated MCP attributes (`serializer.validated_data`): `None` means
/// the field was omitted (PATCH leaves it untouched; create falls back
/// to the model default).
#[derive(Debug, Default, PartialEq)]
struct McpAttrs {
    name: Option<String>,
    url: Option<String>,
    auth_header: Option<String>,
    is_enabled: Option<bool>,
}

/// Validate a create (full) or patch (partial) body. Errors render as
/// DRF does: `{field: [messages]}` in serializer field order, 400.
fn validate_mcp(data: &Map<String, Value>, partial: bool) -> Result<McpAttrs, Map<String, Value>> {
    let mut errors = Map::new();
    let mut attrs = McpAttrs::default();
    // Writable fields in serializer declaration order; read-only and
    // unknown keys are ignored, never errors (verified live).
    if let Some(value) = validate_char_field(data, "name", 80, false, partial, &mut errors) {
        match validate_mcp_name(&value) {
            Ok(name) => attrs.name = Some(name),
            Err(message) => push_error(&mut errors, "name", message),
        }
    }
    if let Some(value) = validate_char_field(data, "url", 500, false, partial, &mut errors) {
        attrs.url = Some(value);
    }
    if let Some(value) = validate_char_field(data, "auth_header", 2048, true, partial, &mut errors)
    {
        attrs.auth_header = Some(value);
    }
    if let Some(enabled) = validate_bool_field(data, "is_enabled", partial, &mut errors) {
        attrs.is_enabled = Some(enabled);
    }
    if errors.is_empty() {
        // The MCP serializer defines no cross-field `validate()`; the
        // URL half below is `URLField` validators + `validate_url`, in
        // order, landing in the same 400 envelope.
        if let Some(url) = attrs.url.clone() {
            if !django_url_valid(&url) {
                push_error(&mut errors, "url", INVALID_URL_MSG);
            } else {
                match normalize_mcp_url(&url) {
                    None => push_error(&mut errors, "url", URL_REQUIRED_MSG),
                    Some(normalized) => match url_scheme_error(&normalized) {
                        Some(message) => push_error(&mut errors, "url", message),
                        None => attrs.url = Some(normalized),
                    },
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(attrs)
    } else {
        Err(errors)
    }
}

/// One `CharField` (`name`, `url`, `auth_header`): required Unless
/// partial, null/type/blank/max-length/nul checks in DRF order, then
/// the stripped value. Returns `None` when omitted or errored.
fn validate_char_field(
    data: &Map<String, Value>,
    field: &str,
    max_length: usize,
    allow_blank: bool,
    partial: bool,
    errors: &mut Map<String, Value>,
) -> Option<String> {
    let Some(raw) = data.get(field) else {
        if !partial && !is_create_optional(field) {
            push_error(errors, field, REQUIRED_MSG);
        }
        return None;
    };
    if raw.is_null() {
        push_error(errors, field, NULL_MSG);
        return None;
    }
    // `run_validation`: `data == ''` or (trim and `str(data).strip()`
    // blank) → blank/''. Only JSON strings can be blank this way —
    // every other JSON type stringifies non-blank in Python.
    if let Some(text) = raw.as_str() {
        if text.is_empty() || text.trim().is_empty() {
            if !allow_blank {
                push_error(errors, field, BLANK_MSG);
                return None;
            }
            return Some(String::new());
        }
    }
    // `to_internal_value`: bools and composites fail; numerics coerce.
    let coerced = match raw {
        Value::String(text) => text.clone(),
        Value::Number(num) => num.to_string(),
        _ => {
            push_error(errors, field, INVALID_STR_MSG);
            return None;
        }
    };
    let stripped: String = coerced.trim().to_string();
    // Validators in order: max length (code points, like Python
    // `len`), nul bytes, then the field's `validate_*`.
    if stripped.chars().count() > max_length {
        push_error(
            errors,
            field,
            &format!("Ensure this field has no more than {max_length} characters."),
        );
        return None;
    }
    if stripped.contains('\0') {
        push_error(errors, field, NULL_CHARS_MSG);
        return None;
    }
    Some(stripped)
}

/// `name`/`url` are required on create; `auth_header`/`is_enabled`
/// fall back to model defaults.
fn is_create_optional(field: &str) -> bool {
    matches!(field, "auth_header" | "is_enabled")
}

/// `validate_name` (`serializers.py:145-149`): strip (already done by
/// `to_internal_value`); a blank value is rejected. Unreachable through
/// the serializer (the blank check fires first — verified live) but
/// kept so the validator reads exactly like the Python.
fn validate_mcp_name(value: &str) -> Result<String, &'static str> {
    let name = value.trim().to_string();
    if name.is_empty() {
        return Err(NAME_REQUIRED_MSG);
    }
    Ok(name)
}

/// `validate_url` (`serializers.py:151-162`): strip, strip trailing
/// slashes, then require a non-empty `http(s)` URL without credentials.
/// Returns `None` when the value collapses to empty (the caller then
/// reports `URL_REQUIRED_MSG`).
fn normalize_mcp_url(value: &str) -> Option<String> {
    let normalized = value.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        return None;
    }
    Some(normalized)
}

/// Whether a normalized URL passes the scheme/credentials half of
/// `validate_url` (the Django `URLField` half is [`django_url_valid`]).
fn url_scheme_error(url: &str) -> Option<&'static str> {
    let scheme_end = url.find(':').unwrap_or(0);
    let scheme = url[..scheme_end].to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Some(URL_HTTP_MSG);
    }
    if url_has_credentials(url, scheme_end + 1) {
        return Some(URL_NO_CREDENTIALS_MSG);
    }
    None
}

/// `parsed.username or parsed.password` over the authority only:
/// rejection needs a non-empty username or password (`http://@host`
/// passes, verified live).
fn url_has_credentials(url: &str, scheme_end: usize) -> bool {
    let authority = url[scheme_end..]
        .strip_prefix("//")
        .map(|rest| rest.split(['/', '?', '#']).next().unwrap_or(""))
        .unwrap_or("");
    let Some(at) = authority.rfind('@') else {
        return false;
    };
    let userinfo = &authority[..at];
    match userinfo.find(':') {
        Some(i) => !userinfo[..i].is_empty() || !userinfo[i + 1..].is_empty(),
        None => !userinfo.is_empty(),
    }
}

/// One `BooleanField` (`is_enabled`): `TRUE_VALUES`/`FALSE_VALUES`
/// (`fields.py:665-684`) — strings fold case first, `1`/`1.0` coerce
/// true by numeric equality, everything else (incl. `''`) is invalid.
/// `None` is null (no `allow_null`); omission is not an error.
fn validate_bool_field(
    data: &Map<String, Value>,
    field: &str,
    partial: bool,
    errors: &mut Map<String, Value>,
) -> Option<bool> {
    let _ = partial;
    let raw = data.get(field)?;
    if raw.is_null() {
        push_error(errors, field, NULL_MSG);
        return None;
    }
    let hit = match raw {
        Value::Bool(hit) => Some(*hit),
        Value::Number(num) => {
            if num.as_i64() == Some(1) || num.as_u64() == Some(1) || num.as_f64() == Some(1.0) {
                Some(true)
            } else if num.as_i64() == Some(0)
                || num.as_u64() == Some(0)
                || num.as_f64() == Some(0.0)
            {
                Some(false)
            } else {
                None
            }
        }
        Value::String(text) => match text.to_ascii_lowercase().as_str() {
            "t" | "y" | "yes" | "true" | "on" | "1" => Some(true),
            "f" | "n" | "no" | "false" | "off" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    };
    match hit {
        Some(value) => Some(value),
        None => {
            push_error(errors, field, INVALID_BOOL_MSG);
            None
        }
    }
}

fn push_error(errors: &mut Map<String, Value>, field: &str, message: &str) {
    errors
        .entry(field.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .expect("error list")
        .push(Value::String(message.to_string()));
}

// ---------------------------------------------------------------------------
// Django URLValidator (validators.py:69-160, DRF URLField)
// ---------------------------------------------------------------------------

/// Whether `value` passes Django's `URLValidator` (the `URLField`
/// validator behind `AssistantMCPServerSerializer.url`, message
/// `INVALID_URL_MSG`).
///
/// Hand-rolled: the `regex` crate is not a dependency of this crate and
/// the foundation `Cargo.toml` files are read-only for port issues.
/// The structure follows `URLValidator.__call__` exactly — unsafe
/// chars, length, scheme allowlist, split, host alternatives, port,
/// path, then the hostname length cap — with two deliberate bounds:
///
/// * the IDN `punycode` fallback is omitted: it can only flip the
///   verdict for a non-ASCII host the first attempt rejects, and no
///   such input was found (every probed unicode host either passes or
///   fails identically; ASCII verdicts are unaffected since
///   `punycode(ASCII) == ASCII`);
/// * `urlsplit` `ValueError`s beyond an unclosed `[` bracket (exotic
///   NFKC cases) fail closed as invalid, like the guard they feed.
pub fn django_url_valid(value: &str) -> bool {
    // Unsafe chars (`\t\r\n`) and the 2048-char validator cap (code
    // points, like Python `len`).
    if value.chars().any(|c| c == '\t' || c == '\r' || c == '\n') {
        return false;
    }
    if value.chars().count() > 2048 {
        return false;
    }
    // Scheme allowlist (`value.split("://")[0].lower()`).
    let scheme = value.split("://").next().unwrap_or("").to_ascii_lowercase();
    if !["http", "https", "ftp", "ftps"].contains(&scheme.as_str()) {
        return false;
    }
    let after_scheme = match value.split_once("://") {
        Some((_, rest)) => rest,
        None => return false,
    };
    // Authority is everything before the path (`[/?#]`).
    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let (authority, path) = after_scheme.split_at(authority_end);
    if path.chars().any(char::is_whitespace) {
        return false;
    }
    // Userinfo (`(?:[^\s:@/]+(?::[^\s:@/]*)?@)?`, greedy = last `@`).
    let hostport = match authority.rfind('@') {
        Some(at) => {
            if !valid_userinfo(&authority[..at]) {
                return false;
            }
            &authority[at + 1..]
        }
        None => authority,
    };
    let Some(host) = split_host_port(hostport) else {
        return false;
    };
    if !valid_django_host(&host) {
        return false;
    }
    // `splitted_url.hostname is None or len(...) > 253`.
    if host.chars().count() > 253 {
        return false;
    }
    true
}

/// `user[:password]` with no whitespace, colon, slash or `@`
/// (`[^\s:@/]+(?::[^\s:@/]*)?`).
fn valid_userinfo(userinfo: &str) -> bool {
    if userinfo.is_empty() {
        return false;
    }
    let (user, password) = match userinfo.split_once(':') {
        Some((user, password)) => (user, Some(password)),
        None => (userinfo, None),
    };
    if user.is_empty() || user.chars().any(userinfo_char_blocked) {
        return false;
    }
    // A second colon would sit inside the password half.
    match password {
        Some(password) => !password.chars().any(userinfo_char_blocked),
        None => true,
    }
}

fn userinfo_char_blocked(c: char) -> bool {
    c.is_whitespace() || c == ':' || c == '@' || c == '/'
}

/// Split `host[:port]`; the port (when present) is 1–5 ASCII digits.
/// Bracketed IPv6 keeps its brackets for the host check.
fn split_host_port(hostport: &str) -> Option<String> {
    if hostport.is_empty() {
        return None;
    }
    if let Some(bracketed) = hostport.strip_prefix('[') {
        let (inner, rest) = bracketed.split_once(']')?;
        let host = format!("[{inner}]");
        if rest.is_empty() {
            return Some(host);
        }
        let port = rest.strip_prefix(':')?;
        if is_empty_or_long(port) {
            return None;
        }
        return Some(format!("{host}:{port}"));
    }
    match hostport.rfind(':') {
        Some(i) => {
            let (host, port) = (&hostport[..i], &hostport[i + 1..]);
            if host.is_empty() || is_empty_or_long(port) {
                return None;
            }
            Some(format!("{host}:{port}"))
        }
        None => Some(hostport.to_string()),
    }
}

fn is_empty_or_long(port: &str) -> bool {
    port.is_empty() || port.len() > 5 || !port.bytes().all(|b| b.is_ascii_digit())
}

/// The host alternatives (`ipv4 | ipv6 | hostname+domain+tld |
/// localhost`), matched case-insensitively like the `re.IGNORECASE`
/// regex. Unicode letters are the `ul` range (`\u00a1-\uffff`):
/// anything at or above `¡` counts (Rust `char` cannot hold
/// surrogates, so the range check is total).
fn valid_django_host(host: &str) -> bool {
    if host.is_empty() {
        return false;
    }
    // Bracketed IPv6 (`split_host_port` keeps the brackets and any
    // `:port` suffix): simple charset first, then the strict address
    // check (`validate_ipv6_address`).
    if let Some(rest) = host.strip_prefix('[') {
        let (inner, after) = match rest.split_once(']') {
            Some(pair) => pair,
            // Unclosed bracket (`urlsplit` raises `ValueError`).
            None => return false,
        };
        if !after.is_empty() {
            let port = match after.strip_prefix(':') {
                Some(port) => port,
                None => return false,
            };
            if is_empty_or_long(port) {
                return false;
            }
        }
        return inner
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
            && inner.parse::<std::net::Ipv6Addr>().is_ok();
    }
    if host.contains('[') || host.contains(']') {
        return false;
    }
    // Split an optional `:port` (unbracketed hosts hold no colon).
    let bare = match host.rfind(':') {
        Some(i) => {
            let (bare, port) = (&host[..i], &host[i + 1..]);
            if is_empty_or_long(port) {
                return false;
            }
            bare
        }
        None => host,
    };
    if bare.is_empty() || bare.contains(':') {
        return false;
    }
    // Dotted IPv4 (exact octet grammar — `01` is not an octet).
    if bare.chars().all(|c| c.is_ascii_digit() || c == '.') && bare.contains('.') {
        return valid_ipv4(bare);
    }
    // `localhost` (any case).
    if bare.eq_ignore_ascii_case("localhost") {
        return true;
    }
    valid_django_hostname(bare)
}

/// `(?:0|25[0-5]|2[0-4][0-9]|1[0-9]?[0-9]?|[1-9][0-9]?)` × 4.
fn valid_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|octet| {
        if octet.is_empty() || octet.len() > 3 || !octet.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        if octet.len() > 1 && octet.starts_with('0') {
            return false; // `0` alone only; `01` is not an octet
        }
        octet.parse::<u32>().is_ok_and(|num| num <= 255)
    })
}

/// `hostname + domain* + tld` (`hostname_re + domain_re + tld_re`).
fn valid_django_hostname(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false; // the TLD half requires a dot
    }
    let (tld, rest) = labels.split_last().expect("two labels");
    // Trailing-dot FQDNs: the empty last label stands for the root, the
    // real TLD is the one before it.
    let (tld, rest) = if tld.is_empty() {
        match rest.split_last() {
            Some((real, rest)) => (real, rest),
            None => return false,
        }
    } else {
        (tld, rest)
    };
    if !valid_tld(tld) {
        return false;
    }
    let (first, middle) = match rest.split_first() {
        Some(pair) => pair,
        None => return false,
    };
    if !valid_first_label(first) {
        return false;
    }
    middle.iter().all(|label| valid_domain_label(label))
}

/// First label: starts/ends alnum, up to 61 dashes inside
/// (`[a-zUL0-9](?:[a-zUL0-9-]{0,61}[a-zUL0-9])?`).
fn valid_first_label(label: &str) -> bool {
    let len = label.chars().count();
    if len == 0 || len > 63 {
        return false;
    }
    let mut chars = label.chars();
    let first = chars.next().expect("non-empty");
    if !is_name_edge(first) {
        return false;
    }
    if len == 1 {
        return true;
    }
    let last = label.chars().next_back().expect("non-empty");
    if !is_name_edge(last) {
        return false;
    }
    label.chars().all(is_name_middle)
}

/// Middle labels: 1–63 chars, no leading/trailing dash.
fn valid_domain_label(label: &str) -> bool {
    valid_first_label(label)
}

/// TLD: `(?:[a-zUL-]{2,63}|xn--[a-z0-9]{1,59})`, no leading/trailing
/// dash (digits only via the punycode form).
fn valid_tld(tld: &str) -> bool {
    if tld.is_empty() || tld.starts_with('-') || tld.ends_with('-') {
        return false;
    }
    // ASCII-folded once: the match is case-insensitive and the punycode
    // prefix is pure ASCII either way.
    let folded = tld.to_ascii_lowercase();
    if let Some(rest) = folded.strip_prefix("xn--") {
        let len = rest.chars().count();
        return (1..=59).contains(&len) && rest.chars().all(|c| c.is_ascii_alphanumeric());
    }
    let len = tld.chars().count();
    (2..=63).contains(&len)
        && tld
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == '-' || is_ul_letter(c))
}

/// Label edge: ASCII alnum or a `ul`-range letter.
fn is_name_edge(c: char) -> bool {
    c.is_ascii_alphanumeric() || is_ul_letter(c)
}

/// Label middle: edge class plus the dash.
fn is_name_middle(c: char) -> bool {
    is_name_edge(c) || c == '-'
}

/// The `ul` range (`\u00a1-\uffff`).
fn is_ul_letter(c: char) -> bool {
    c >= '¡'
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// One `assistant_mcp_server` row (the columns the views touch).
struct ServerRow {
    id: uuid::Uuid,
    name: String,
    url: String,
    auth_header_encrypted: Option<Vec<u8>>,
    is_enabled: bool,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

/// Serialize a row, optionally with the prefix its tools actually get
/// (`_serialize`, `mcp_servers.py:26-34`).
///
/// `tool_prefix` is the row's own slug; the prefix a run assigns can
/// differ, because slugification is lossy and colliding servers are
/// disambiguated with a counter. Showing the raw slug tells two
/// colliding servers they share a prefix when they do not, so the list
/// view resolves it (see `list_servers`).
fn serialize_server(row: &ServerRow, effective_prefix: Option<&str>) -> Value {
    let mut body = Map::with_capacity(9);
    body.insert("id".to_string(), Value::String(row.id.to_string()));
    body.insert("name".to_string(), Value::String(row.name.clone()));
    body.insert("url".to_string(), Value::String(row.url.clone()));
    body.insert(
        "has_auth_header".to_string(),
        Value::Bool(
            pidash_db::assistant::models::assistant_mcp_server::has_auth_header(
                &row.auth_header_encrypted,
            ),
        ),
    );
    body.insert(
        "tool_prefix".to_string(),
        Value::String(
            pidash_db::assistant::models::assistant_mcp_server::tool_prefix(&row.name, &row.id),
        ),
    );
    body.insert("is_enabled".to_string(), Value::Bool(row.is_enabled));
    body.insert(
        "created_at".to_string(),
        Value::String(crate::serializer::render_datetime(&row.created_at)),
    );
    body.insert(
        "updated_at".to_string(),
        Value::String(crate::serializer::render_datetime(&row.updated_at)),
    );
    body.insert(
        "effective_tool_prefix".to_string(),
        effective_prefix
            .map(|prefix| Value::String(prefix.to_string()))
            .unwrap_or(Value::Null),
    );
    Value::Object(body)
}

/// `{"error":"url_blocked", ...}` 400 (`_blocked_response`, `:37-41`).
fn blocked_response() -> Response {
    error_response(
        StatusCode::BAD_REQUEST,
        &serde_json::json!({
            "error": "url_blocked",
            "detail": "That server host is not allowed.",
        }),
    )
}

/// `{"error":"duplicate_name", ...}` 400
/// (`_duplicate_name_response`, `:44-48`).
fn duplicate_name_response() -> Response {
    error_response(
        StatusCode::BAD_REQUEST,
        &serde_json::json!({
            "error": "duplicate_name",
            "detail": "You already have a tool server with that name.",
        }),
    )
}

/// Empty 404: `Response(status=404)` with no data (DRF renders `b''`)
/// — the `_owned` miss (`:124-125`).
fn empty_not_found() -> Response {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::empty())
        .expect("empty 404")
}

/// Parse a JSON object body: empty means `{}`; anything else must parse
/// as JSON and be an object (DRF `ParseError` / non-dict shapes).
#[allow(clippy::result_large_err)]
fn parse_object_body(raw: &[u8]) -> Result<Map<String, Value>, Response> {
    if raw.is_empty() {
        return Ok(Map::new());
    }
    let value: Value = match serde_json::from_slice(raw) {
        Ok(value) => value,
        Err(err) => {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                &serde_json::json!({"detail": format!("JSON parse error - {err}")}),
            ));
        }
    };
    match value {
        Value::Object(fields) => Ok(fields),
        Value::Null => Err(validation_error_response(&dtype_error("NoneType"))),
        Value::Bool(_) => Err(validation_error_response(&dtype_error("bool"))),
        Value::Number(num) => {
            let kind = if num.is_i64() || num.is_u64() {
                "int"
            } else {
                "float"
            };
            Err(validation_error_response(&dtype_error(kind)))
        }
        Value::String(_) => Err(validation_error_response(&dtype_error("str"))),
        Value::Array(_) => Err(validation_error_response(&dtype_error("list"))),
    }
}

/// `{"non_field_errors": ["Invalid data. Expected a dictionary, but got
/// {kind}."]}` 400 (DRF `BaseSerializer.to_internal_value`).
fn dtype_error(kind: &str) -> Map<String, Value> {
    let mut errors = Map::new();
    errors.insert(
        "non_field_errors".to_string(),
        Value::Array(vec![Value::String(format!(
            "Invalid data. Expected a dictionary, but got {kind}."
        ))]),
    );
    errors
}

/// Render a field-error dict 400 (`raise_exception=True`).
fn validation_error_response(errors: &Map<String, Value>) -> Response {
    error_response(StatusCode::BAD_REQUEST, &Value::Object(errors.clone()))
}

/// Set/clear the encrypted auth header (`_apply_auth_header`,
/// `:51-67`): an explicitly empty string clears the stored header;
/// omitting the field entirely leaves it untouched (so a PATCH that
/// only renames the server does not silently drop its credential). A
/// crypto `AssistantError` renders as its code/detail/status; an
/// operational failure propagates to the generic 500.
#[allow(clippy::result_large_err)]
fn apply_auth_header(
    encrypted: &mut Option<Vec<u8>>,
    auth_header: Option<&str>,
) -> Result<(), Response> {
    let Some(header) = auth_header else {
        return Ok(());
    };
    if header.is_empty() {
        *encrypted = None;
        return Ok(());
    }
    match super::encrypt_secret(header) {
        Ok(token) => {
            *encrypted = Some(token);
            Ok(())
        }
        Err(super::SecretError::Assistant(error)) => Err(super::assistant_error_response(&error)),
        Err(super::SecretError::Transport(_)) => Err(Denial::ServerError.into_response()),
    }
}

/// Whether `url` fails the SSRF guard for this deployment.
fn url_blocked(state: &AppState, url: &str) -> bool {
    pidash_services::assistant::ssrf::is_blocked(
        url,
        state.settings().assistant.block_private_urls,
        &pidash_services::assistant::ssrf::SystemResolver,
    )
}

/// The operator server cap (`mcp.max_servers()`, `:94`):
/// `int(ASSISTANT_MCP_MAX_SERVERS or DEFAULT_MAX_SERVERS)`. An
/// unparseable operator value falls back to the default rather than
/// 500ing every create (unexercised; documented).
fn server_cap() -> usize {
    let override_value = std::env::var("ASSISTANT_MCP_MAX_SERVERS")
        .ok()
        .and_then(|raw| raw.parse::<i64>().ok());
    pidash_services::assistant::mcp::max_servers(override_value)
}

/// `GET`: every own row in `created_at` order, resolved over the
/// *enabled* set exactly as a run does — a disabled server claims no
/// prefix, so it cannot push an enabled one onto a counter suffix it
/// would never actually get (`:71-77`).
async fn list_servers(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match request_actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let rows = match fetch_servers(&pool, &actor.id).await {
        Ok(rows) => rows,
        Err(denial) => return denial.into_response(),
    };
    let enabled: Vec<(String, String)> = rows
        .iter()
        .filter(|row| row.is_enabled)
        .map(|row| {
            (
                row.id.to_string(),
                pidash_db::assistant::models::assistant_mcp_server::tool_prefix(&row.name, &row.id),
            )
        })
        .collect();
    let effective = pidash_services::assistant::mcp::unique_prefixes(&enabled);
    let body: Vec<Value> = rows
        .iter()
        .map(|row| serialize_server(row, effective.get(&row.id.to_string()).map(String::as_str)))
        .collect();
    json_body(StatusCode::OK, &Value::Array(body))
}

/// `POST`: validate, SSRF-guard, duplicate-name check, server-cap
/// check, encrypt the header, save (`:79-120`). The unique constraint
/// is the authority on the pre-check race (`IntegrityError` →
/// `duplicate_name`).
async fn create_server(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: Body,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match request_actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let raw = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return Denial::ServerError.into_response(),
    };
    let data = match parse_object_body(&raw) {
        Ok(data) => data,
        Err(response) => return response,
    };
    let attrs = match validate_mcp(&data, false) {
        Ok(attrs) => attrs,
        Err(errors) => return validation_error_response(&errors),
    };
    // Names/URLs are validated above, so both are present on create.
    let (Some(name), Some(url)) = (attrs.name.clone(), attrs.url.clone()) else {
        return Denial::ServerError.into_response();
    };
    if url_blocked(&state, &url) {
        return blocked_response();
    }
    match name_taken(&pool, &actor.id, &name, None).await {
        Ok(true) => return duplicate_name_response(),
        Ok(false) => {}
        Err(denial) => return denial.into_response(),
    }
    // Toolsets are entered sequentially at the start of every turn,
    // each bounded only by the connect timeout, so an unbounded server
    // list is dead time the user pays on every message they send.
    // Refuse here rather than let the run-time cap silently drop the
    // newest server.
    let limit = server_cap();
    match server_count(&pool, &actor.id).await {
        Ok(count) if count >= limit as i64 => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &serde_json::json!({
                    "error": "too_many_servers",
                    "detail": format!(
                        "You can have at most {limit} tool servers. Remove one before adding another."
                    ),
                }),
            );
        }
        Ok(_) => {}
        Err(denial) => return denial.into_response(),
    }
    let mut encrypted: Option<Vec<u8>> = None;
    if let Err(response) = apply_auth_header(&mut encrypted, attrs.auth_header.as_deref()) {
        return response;
    }
    let id = uuid::Uuid::new_v4();
    let now = micros_now();
    let saved = sqlx::query(
        "INSERT INTO \"assistant_mcp_server\" \
         (\"id\", \"user_id\", \"name\", \"url\", \"auth_header_encrypted\", \
          \"is_enabled\", \"created_at\", \"updated_at\") \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $7) \
         RETURNING \"created_at\", \"updated_at\"",
    )
    .bind(id)
    .bind(actor.id)
    .bind(&name)
    .bind(&url)
    .bind(&encrypted)
    .bind(attrs.is_enabled.unwrap_or(true))
    .bind(now)
    .fetch_one(&pool)
    .await;
    use sqlx::Row as _;
    let saved = match saved {
        Ok(row) => row,
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            // Concurrent create with the same name: the pre-check above
            // raced. The unique constraint is the authority; answer as
            // the pre-check would have.
            return duplicate_name_response();
        }
        Err(_) => return Denial::ServerError.into_response(),
    };
    json_body(
        StatusCode::CREATED,
        &serialize_server(
            &ServerRow {
                id,
                name,
                url,
                auth_header_encrypted: encrypted,
                is_enabled: attrs.is_enabled.unwrap_or(true),
                created_at: saved.try_get("created_at").unwrap_or(now),
                updated_at: saved.try_get("updated_at").unwrap_or(now),
            },
            None,
        ),
    )
}

/// `PATCH`: own-row lookup (404 on miss), partial validation, SSRF only
/// when the URL actually changes, duplicate-name only when the name
/// does, then save (`:127-158`). The `IntegrityError` branch answers
/// `duplicate_name` like the pre-check would have.
async fn patch_server(
    State(state): State<AppState>,
    Path(server_id_raw): Path<String>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: Body,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match request_actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    // Django's `<uuid:server_id>` converter 404s on garbage before the
    // view runs (the space-handler precedent maps that to the envelope).
    let server_id: uuid::Uuid = match server_id_raw.parse() {
        Ok(id) => id,
        Err(_) => return empty_not_found(),
    };
    let raw = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return Denial::ServerError.into_response(),
    };
    let data = match parse_object_body(&raw) {
        Ok(data) => data,
        Err(response) => return response,
    };
    let attrs = match validate_mcp(&data, true) {
        Ok(attrs) => attrs,
        Err(errors) => return validation_error_response(&errors),
    };
    let Some(mut row) = (match fetch_server(&pool, &actor.id, &server_id).await {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    }) else {
        return empty_not_found();
    };
    let url = attrs.url.clone().unwrap_or_else(|| row.url.clone());
    if url != row.url && url_blocked(&state, &url) {
        return blocked_response();
    }
    let name = attrs.name.clone().unwrap_or_else(|| row.name.clone());
    if name != row.name {
        match name_taken(&pool, &actor.id, &name, Some(&server_id)).await {
            Ok(true) => return duplicate_name_response(),
            Ok(false) => {}
            Err(denial) => return denial.into_response(),
        }
    }
    row.name = name;
    row.url = url;
    if let Some(is_enabled) = attrs.is_enabled {
        row.is_enabled = is_enabled;
    }
    if let Err(response) =
        apply_auth_header(&mut row.auth_header_encrypted, attrs.auth_header.as_deref())
    {
        return response;
    }
    row.updated_at = micros_now();
    let saved = sqlx::query(
        "UPDATE \"assistant_mcp_server\" SET \"name\" = $1, \"url\" = $2, \
         \"is_enabled\" = $3, \"auth_header_encrypted\" = $4, \"updated_at\" = $5 \
         WHERE \"id\" = $6 AND \"user_id\" = $7",
    )
    .bind(&row.name)
    .bind(&row.url)
    .bind(row.is_enabled)
    .bind(&row.auth_header_encrypted)
    .bind(row.updated_at)
    .bind(server_id)
    .bind(actor.id)
    .execute(&pool)
    .await;
    match saved {
        Ok(_) => {}
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            return duplicate_name_response();
        }
        Err(_) => return Denial::ServerError.into_response(),
    }
    json_body(StatusCode::OK, &serialize_server(&row, None))
}

/// `DELETE`: own-row lookup (404 on miss), delete, empty 204
/// (`:160-164`).
async fn delete_server(
    State(state): State<AppState>,
    Path(server_id_raw): Path<String>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match request_actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let server_id: uuid::Uuid = match server_id_raw.parse() {
        Ok(id) => id,
        Err(_) => return empty_not_found(),
    };
    let owned = match fetch_server(&pool, &actor.id, &server_id).await {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    if owned.is_none() {
        return empty_not_found();
    }
    let deleted =
        sqlx::query("DELETE FROM \"assistant_mcp_server\" WHERE \"id\" = $1 AND \"user_id\" = $2")
            .bind(server_id)
            .bind(actor.id)
            .execute(&pool)
            .await;
    match deleted {
        Ok(_) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::empty())
            .expect("empty 204"),
        Err(_) => Denial::ServerError.into_response(),
    }
}

/// `timezone.now()` at DB precision: Postgres `timestamptz` stores
/// microseconds, so the stamp is rounded to 6 digits before insert
/// (what the row reads back afterwards).
fn micros_now() -> chrono::DateTime<chrono::Utc> {
    use chrono::SubsecRound as _;
    chrono::Utc::now().round_subsecs(6)
}

/// Every own row in `Meta.ordering` (`created_at` ascending).
async fn fetch_servers(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
) -> Result<Vec<ServerRow>, Denial> {
    let rows = sqlx::query(
        "SELECT \"id\", \"name\", \"url\", \"auth_header_encrypted\", \
         \"is_enabled\", \"created_at\", \"updated_at\" \
         FROM \"assistant_mcp_server\" WHERE \"user_id\" = $1 \
         ORDER BY \"created_at\" ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    use sqlx::Row as _;
    rows.iter()
        .map(|row| {
            Ok(ServerRow {
                id: row.try_get("id").map_err(|_| Denial::ServerError)?,
                name: row.try_get("name").map_err(|_| Denial::ServerError)?,
                url: row.try_get("url").map_err(|_| Denial::ServerError)?,
                auth_header_encrypted: row
                    .try_get("auth_header_encrypted")
                    .map_err(|_| Denial::ServerError)?,
                is_enabled: row.try_get("is_enabled").map_err(|_| Denial::ServerError)?,
                created_at: row.try_get("created_at").map_err(|_| Denial::ServerError)?,
                updated_at: row.try_get("updated_at").map_err(|_| Denial::ServerError)?,
            })
        })
        .collect()
}

/// `_owned`: `.filter(id=…, user=…).first()` (`:124-125`).
async fn fetch_server(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
    server_id: &uuid::Uuid,
) -> Result<Option<ServerRow>, Denial> {
    let rows = sqlx::query(
        "SELECT \"id\", \"name\", \"url\", \"auth_header_encrypted\", \
         \"is_enabled\", \"created_at\", \"updated_at\" \
         FROM \"assistant_mcp_server\" WHERE \"id\" = $1 AND \"user_id\" = $2",
    )
    .bind(server_id)
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    use sqlx::Row as _;
    rows.first()
        .map(|row| {
            Ok(ServerRow {
                id: row.try_get("id").map_err(|_| Denial::ServerError)?,
                name: row.try_get("name").map_err(|_| Denial::ServerError)?,
                url: row.try_get("url").map_err(|_| Denial::ServerError)?,
                auth_header_encrypted: row
                    .try_get("auth_header_encrypted")
                    .map_err(|_| Denial::ServerError)?,
                is_enabled: row.try_get("is_enabled").map_err(|_| Denial::ServerError)?,
                created_at: row.try_get("created_at").map_err(|_| Denial::ServerError)?,
                updated_at: row.try_get("updated_at").map_err(|_| Denial::ServerError)?,
            })
        })
        .transpose()
}

/// `.filter(user=…, name=…).exists()`, excluding one row for renames.
async fn name_taken(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
    name: &str,
    exclude: Option<&uuid::Uuid>,
) -> Result<bool, Denial> {
    use sqlx::Row as _;
    let row = if let Some(exclude) = exclude {
        sqlx::query(
            "SELECT EXISTS(SELECT 1 FROM \"assistant_mcp_server\" \
             WHERE \"user_id\" = $1 AND \"name\" = $2 AND \"id\" <> $3)",
        )
        .bind(user_id)
        .bind(name)
        .bind(exclude)
        .fetch_one(pool)
        .await
    } else {
        sqlx::query(
            "SELECT EXISTS(SELECT 1 FROM \"assistant_mcp_server\" \
             WHERE \"user_id\" = $1 AND \"name\" = $2)",
        )
        .bind(user_id)
        .bind(name)
        .fetch_one(pool)
        .await
    };
    row.map_err(|_| Denial::ServerError)?
        .try_get::<bool, _>("exists")
        .map_err(|_| Denial::ServerError)
}

/// `.filter(user=…).count()` for the server cap.
async fn server_count(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<i64, Denial> {
    use sqlx::Row as _;
    sqlx::query("SELECT COUNT(*) AS \"count\" FROM \"assistant_mcp_server\" WHERE \"user_id\" = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)?
        .try_get("count")
        .map_err(|_| Denial::ServerError)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(json: serde_json::Value) -> Map<String, Value> {
        json.as_object().expect("object").clone()
    }

    #[test]
    fn create_requires_name_and_url() {
        let errors = validate_mcp(&fields(serde_json::json!({})), false).expect_err("errors");
        assert_eq!(
            errors,
            fields(serde_json::json!({
                "name": ["This field is required."],
                "url": ["This field is required."],
            }))
        );
    }

    #[test]
    fn blank_and_whitespace_are_blank() {
        for raw in ["", "   "] {
            let errors = validate_mcp(
                &fields(serde_json::json!({"name": raw, "url": "https://8.8.8.8/mcp"})),
                false,
            )
            .expect_err("errors");
            assert_eq!(
                errors.get("name").expect("name error"),
                &serde_json::json!(["This field may not be blank."])
            );
        }
        let errors = validate_mcp(
            &fields(serde_json::json!({"name": "n", "url": "   "})),
            false,
        )
        .expect_err("errors");
        assert_eq!(
            errors.get("url").expect("url error"),
            &serde_json::json!(["This field may not be blank."])
        );
    }

    #[test]
    fn django_url_cases_match_live_probes() {
        // (input, valid-django?, stored-or-message)
        let cases = [
            ("https://8.8.8.8/mcp", true, "https://8.8.8.8/mcp"),
            ("https://8.8.8.8/mcp/", true, "https://8.8.8.8/mcp"),
            (
                "ftp://tools.example.com/mcp",
                true,
                "url must be an http(s) URL.",
            ),
            ("javascript:alert(1)", false, "Enter a valid URL."),
            (
                "https://user:pw@8.8.8.8/mcp",
                true,
                "url must not contain credentials.",
            ),
            (
                "http://user@8.8.8.8/mcp",
                true,
                "url must not contain credentials.",
            ),
            ("http://:pw@8.8.8.8/mcp", false, "Enter a valid URL."),
            ("http://x/mcp", false, "Enter a valid URL."),
            ("http://a.bc/mcp", true, "http://a.bc/mcp"),
            ("http://8.8.8.8./mcp", false, "Enter a valid URL."),
            ("http://8.8.8.8:1/mcp", true, "http://8.8.8.8:1/mcp"),
            ("http://8.8.8.8:123456/mcp", false, "Enter a valid URL."),
            ("http://[::1]/mcp", true, "http://[::1]/mcp"),
            ("http://[::1", false, "Enter a valid URL."),
            ("http://-bad.com/mcp", false, "Enter a valid URL."),
            ("http://bad-.com/mcp", false, "Enter a valid URL."),
            ("http://foo_bar/mcp", false, "Enter a valid URL."),
            ("http://münchen.de/mcp", true, "http://münchen.de/mcp"),
            (
                "http://localhost:9000/mcp",
                true,
                "http://localhost:9000/mcp",
            ),
            ("https://8.8.8.8/mcp withspace", false, "Enter a valid URL."),
            ("HTTP://8.8.8.8/MCP", true, "HTTP://8.8.8.8/MCP"),
            ("ftp://a.bc/x", true, "url must be an http(s) URL."),
            ("gopher://a.bc/x", false, "Enter a valid URL."),
            ("example.com", false, "Enter a valid URL."),
            ("http:///mcp", false, "Enter a valid URL."),
            ("http://01.02.03.04/mcp", false, "Enter a valid URL."),
            ("http://8.8.8.8:abc/mcp", false, "Enter a valid URL."),
        ];
        for (input, django_ok, stored_or_message) in cases {
            assert_eq!(django_url_valid(input), django_ok, "{input}");
            let outcome = validate_mcp(
                &fields(serde_json::json!({"name": "n", "url": input})),
                false,
            );
            if django_ok && !stored_or_message.contains(' ') {
                let attrs = outcome.expect("valid");
                assert_eq!(attrs.url.as_deref(), Some(stored_or_message), "{input}");
            } else {
                let errors = outcome.expect_err("invalid");
                let messages = errors
                    .get("url")
                    .expect("url error")
                    .as_array()
                    .expect("list");
                assert!(
                    messages
                        .iter()
                        .any(|m| m.as_str() == Some(stored_or_message)),
                    "{input}: {messages:?}"
                );
            }
        }
    }

    #[test]
    fn boolean_coercion_matches_drf() {
        let check = |value: Value| {
            validate_mcp(
                &fields(serde_json::json!({
                    "name": "n", "url": "https://8.8.8.8/mcp", "is_enabled": value,
                })),
                false,
            )
            .map(|attrs| attrs.is_enabled)
        };
        assert_eq!(check(serde_json::json!(true)), Ok(Some(true)));
        assert_eq!(check(serde_json::json!("TRUE")), Ok(Some(true)));
        assert_eq!(check(serde_json::json!("off")), Ok(Some(false)));
        assert_eq!(check(serde_json::json!(1)), Ok(Some(true)));
        assert_eq!(check(serde_json::json!(0.0)), Ok(Some(false)));
        assert!(check(serde_json::json!("maybe")).is_err());
        assert!(check(serde_json::json!("")).is_err());
        assert!(check(serde_json::json!(1.5)).is_err());
        assert!(check(serde_json::json!(null)).is_err());
        assert!(check(serde_json::json!([])).is_err());
    }

    #[test]
    fn numerics_coerce_and_unknown_keys_ignored() {
        let attrs = validate_mcp(
            &fields(serde_json::json!({
                "name": 123, "url": "https://8.8.8.8/mcp",
                "tool_prefix": "z", "id": "q", "unknown": 1,
            })),
            false,
        )
        .expect("valid");
        assert_eq!(attrs.name.as_deref(), Some("123"));
        assert!(validate_mcp(
            &fields(serde_json::json!({"name": true, "url": "https://8.8.8.8/mcp"})),
            false
        )
        .is_err());
    }

    #[test]
    fn max_lengths_fire_in_order() {
        let errors = validate_mcp(
            &fields(serde_json::json!({
                "name": "n", "url": "https://8.8.8.8/mcp",
                "auth_header": "x".repeat(2049),
            })),
            false,
        )
        .expect_err("errors");
        assert_eq!(
            errors.get("auth_header").expect("header error"),
            &serde_json::json!(["Ensure this field has no more than 2048 characters."])
        );
    }

    #[test]
    fn partial_patch_skips_missing() {
        let attrs = validate_mcp(&fields(serde_json::json!({})), true).expect("valid");
        assert_eq!(attrs, McpAttrs::default());
        let errors =
            validate_mcp(&fields(serde_json::json!({"name": "   "})), true).expect_err("errors");
        assert!(errors.contains_key("name"));
    }

    #[test]
    fn non_dict_bodies_match_drf() {
        for (raw, kind) in [
            ("[1,2]", "list"),
            ("\"str\"", "str"),
            ("5", "int"),
            ("1.5", "float"),
            ("true", "bool"),
            ("null", "NoneType"),
        ] {
            let response = parse_object_body(raw.as_bytes()).expect_err("invalid");
            let _ = (response, kind);
        }
        assert!(parse_object_body(b"").expect("empty").is_empty());
        // The messages themselves:
        let errors = validate_non_dict(&serde_json::json!([1, 2]));
        assert_eq!(
            errors.get("non_field_errors").expect("errors"),
            &serde_json::json!(["Invalid data. Expected a dictionary, but got list."])
        );
    }

    fn validate_non_dict(value: &Value) -> Map<String, Value> {
        match value {
            Value::Array(_) => dtype_error("list"),
            _ => dtype_error("other"),
        }
    }

    #[test]
    fn serialize_shape_orders_keys() {
        let id = uuid::Uuid::parse_str("12345678-1234-5678-1234-567812345678").expect("uuid");
        let row = ServerRow {
            id,
            name: "Tools".to_string(),
            url: "https://8.8.8.8/mcp".to_string(),
            auth_header_encrypted: None,
            is_enabled: true,
            created_at: chrono::DateTime::parse_from_rfc3339("2026-09-29T12:00:00Z")
                .expect("time")
                .with_timezone(&chrono::Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339("2026-09-29T12:00:01Z")
                .expect("time")
                .with_timezone(&chrono::Utc),
        };
        let rendered =
            serde_json::to_string(&serialize_server(&row, Some("mcp_tools"))).expect("json");
        assert_eq!(
            rendered,
            "{\"id\":\"12345678-1234-5678-1234-567812345678\",\"name\":\"Tools\",\
             \"url\":\"https://8.8.8.8/mcp\",\"has_auth_header\":false,\
             \"tool_prefix\":\"mcp_tools\",\"is_enabled\":true,\
             \"created_at\":\"2026-09-29T12:00:00Z\",\"updated_at\":\"2026-09-29T12:00:01Z\",\
             \"effective_tool_prefix\":\"mcp_tools\"}"
        );
        let nulled = serde_json::to_string(&serialize_server(&row, None)).expect("json");
        assert!(nulled.ends_with("\"effective_tool_prefix\":null}"));
    }
}
