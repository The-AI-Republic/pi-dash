#![forbid(unsafe_code)]

//! Prompt-section REST surface (D-04, stage 4).
//!
//! Ports (all under `apps/api/pi_dash/prompting/`):
//!
//! * `views.py:57-62` (`WORKSPACE_ADMIN_ROLE`, scope consts) —
//!   [`WORKSPACE_ADMIN_ROLE`], [`SCOPE_USER`], [`SCOPE_WORKSPACE`].
//! * `views.py:65-72` (`_FakeRun`) — the preview stub run: a fresh UUID,
//!   no trigger, `local_runner` executor, empty tool plan. No struct: the
//!   preview builders inline its three observable values (`run_id`,
//!   `trigger: None`, `executor_kind: "local_runner"`, attempt from the
//!   live `agent_run` count).
//! * `views.py:74-99` (`_is_workspace_admin`, `_is_workspace_member`,
//!   `_get_workspace_or_404`, `_resolve_scope`) — [`is_workspace_admin`],
//!   [`is_workspace_member`], [`workspace_or_404`], [`resolve_scope`].
//!   Superusers short-circuit both role checks; the member/admin
//!   `EXISTS` queries carry the soft-delete (`deleted_at IS NULL`) and
//!   `is_active` guards of the default manager (fixture `FIX-guards`
//!   `sql.is_admin_exists` / `is_member_exists`).
//! * `views.py:102-146` (`_section_breakdown`) — [`section_breakdown`]:
//!   one override-index load, per-section `resolve_section` over the
//!   services composer, `needs_attention` from the row that actually
//!   resolved, capability flags from the *effective* tier.
//! * `views.py:149-177` (`PromptSectionListEndpoint.get`) — [`list_sections`].
//! * `views.py:186-190` (`_check_write_permission`) —
//!   [`check_write_permission`]: workspace scope is admin-only, user scope
//!   is any member (own row enforced by `user = self`).
//! * `views.py:192-243` (PUT) — [`upsert_section`]: scope from
//!   body-then-query, registry 404, tier gates, body-required 400,
//!   `validate_override` 400, upsert 200.
//! * `views.py:245-290` (`_upsert`, `_apply_update`) — [`upsert_row`]:
//!   `SELECT … FOR UPDATE` inside a transaction, in-place bump with
//!   `version = version + 1` (the `F("version") + 1` without
//!   read-modify-write drift), `needs_attention = false`; the lost-create
//!   race retries once after a unique violation, exactly like the
//!   `IntegrityError` branch.
//! * `views.py:292-319` (DELETE) — [`delete_override`]: soft-deactivate
//!   (`is_active = false`), 204 with an empty body.
//! * `views.py:322-357` (`PromptCompiledEndpoint.get`) —
//!   [`compiled_template`], including the dual workspace-only
//!   `automatic_template_body` when a user-scope resolution carries a
//!   `user:` source.
//! * `views.py:360-439` (`PromptPreviewEndpoint.post`) — [`preview_prompt`]:
//!   admin-or-member gate, member draft restriction to user scope,
//!   `_issue_context` / `_scheduler_context` ([`issue_context`],
//!   [`scheduler_context`]), unsaved-draft tier gate, `compose`, and the
//!   422 render-failed branch.
//! * `views.py:441-483` (`_issue_context`, `_scheduler_context`) —
//!   [`load_issue_context`], [`load_scheduler_context`]: the issue /
//!   binding rows plus every relation the services context builders take
//!   as preloaded views.
//! * `urls.py:13-37` (the 4 routes) — [`routes`].
//!
//! Layering: serializer shapes live in
//! `pidash_services::prompting::shape`, resolution in
//! `pidash_services::prompting::composer`, validation in
//! `pidash_services::prompting::validation`, context assembly in
//! `pidash_services::prompting::context`. This module owns the HTTP shell
//! (routes, session auth, the workspace-role gates), the SQL text, and
//! the response rendering.
//!
//! Auth order mirrors DRF: `IsAuthenticated` (Django-session authN via
//! [`crate::license::resolve_actor`]) answers 401 before any gate or
//! method logic; the workspace lookup 404s before the role gate, so an
//! unknown slug is 404 even for outsiders.
//!
//! Registration is the cutover granularity (same rule as the `app_issues`,
//! `license`, `space` and `loop` families): the owned methods serve from
//! Rust while every other method on those paths proxies to Django, so
//! DRF's authenticate-before-method order (401-anon, 405-after-auth,
//! metadata OPTIONS) is preserved byte for byte. `HEAD` rides axum's
//! `get` handling on the GET paths like Django's `GET`-backed `HEAD`; on
//! the PUT/DELETE detail path and the POST preview path (no GET in
//! Django) `HEAD` proxies so Django's own 405 answers.
//!
//! [`routes`] merges the four owned paths; nothing else.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-status (`views.py:243`): PUT create returns `200`, not `201`,
//!   with the read-serializer body.
//! * `_related_context` (`context.py:110-146`) has no soft-delete-target
//!   filter in Python (only the relation rows go through the default
//!   manager), so a `relates_to` link to a soft-deleted issue still
//!   renders there — while the directional builder (`context.py:171-218`)
//!   explicitly excludes soft-deleted targets. This port follows the
//!   services context contract instead (both skip deleted targets), and
//!   the difference is recorded here: a `relates_to` row pointing at a
//!   soft-deleted issue renders `[]` from Rust where Django renders the
//!   ref. No fixture covers it (`FIX-handlers` has no deleted-target
//!   `relates_to` case).

use std::collections::{BTreeMap, HashMap, HashSet};

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::Value;

use crate::state::AppState;

use pidash_services::prompting::{composer, context, recipes, registry, validation};

/// `views.py:59`: numeric `WorkspaceMember.role` for the admin role.
pub const WORKSPACE_ADMIN_ROLE: i32 = 20;
/// `views.py:61`: personal resolution scope.
pub const SCOPE_USER: &str = "user";
/// `views.py:62`: workspace resolution scope.
pub const SCOPE_WORKSPACE: &str = "workspace";

/// `urls.py:18` (under the `api/` include).
pub const SECTION_LIST_PATH: &str = "/api/workspaces/{slug}/prompt-sections";
/// `urls.py:23` (under the `api/` include).
pub const SECTION_DETAIL_PATH: &str = "/api/workspaces/{slug}/prompt-sections/{section_key}";
/// `urls.py:28` (under the `api/` include).
pub const COMPILED_PATH: &str = "/api/workspaces/{slug}/prompts/{kind}/compiled";
/// `urls.py:33` (under the `api/` include).
pub const PREVIEW_PATH: &str = "/api/workspaces/{slug}/prompts/{kind}/preview";

/// Register the four owned prompting paths. Sibling paths stay unmatched
/// and proxy to Django through the fallback.
pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route(SECTION_LIST_PATH, owned_list(get(list_sections)))
        .route(
            SECTION_DETAIL_PATH,
            owned_detail(axum::routing::put(upsert_section).delete(delete_override)),
        )
        .route(COMPILED_PATH, owned_compiled(get(compiled_template)))
        .route(PREVIEW_PATH, owned_preview(post(preview_prompt)))
}

/// The section-list path: only GET exists in Django, so only GET is
/// owned. `HEAD` rides axum's `get` handling like Django's `GET`-backed
/// `HEAD`; everything else proxies (DRF's 405-after-auth and metadata).
fn owned_list(
    owned: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// The section-detail path: only PUT + DELETE exist in Django. GET, HEAD
/// and the rest proxy so Django answers its own 405-after-auth.
fn owned_detail(
    owned: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned
        .get(crate::edge::proxy)
        .head(crate::edge::proxy)
        .post(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// The compiled path: only GET exists in Django.
fn owned_compiled(
    owned: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// The preview path: only POST exists in Django. GET/HEAD and the rest
/// proxy so Django answers its own 405-after-auth.
fn owned_preview(
    owned: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned
        .get(crate::edge::proxy)
        .head(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// Handler failure with its exact status + body.
#[derive(Debug)]
enum Denial {
    /// 401, DRF `NotAuthenticated` (anonymous on a guarded endpoint).
    Unauthorized,
    /// 403, `{"error":"forbidden"}` (member / write-permission / scope gates).
    Forbidden,
    /// 404, `{"error":"workspace not found"}`.
    WorkspaceNotFound,
    /// 400, `{"error":…}` (view-inline guard bodies).
    BadError(String),
    /// 400, `{"detail":…}` (DRF `ParseError`, unparseable body).
    BadDetail(String),
    /// 400, `{"error":…, "kinds":[…]}` (unknown kind).
    BadKind(String),
    /// 404, `{"error":…}` (unknown section / no active override).
    NotError(String),
    /// 403, `{"error":…}` (tier gates / draft gates).
    ForbiddenError(String),
    /// 400, `{"error":"override validation failed","detail":…}`.
    ValidationFailed(String),
    /// 422, `{"error":"render failed","detail":…}` (preview render).
    RenderFailed(String),
    /// 403, `{"detail":"CSRF Failed: …"}` (session-auth CSRF enforcement
    /// on unsafe methods).
    CsrfFailed(String),
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                crate::license::UNAUTHENTICATED_BODY.to_owned(),
            ),
            Denial::Forbidden => (StatusCode::FORBIDDEN, r#"{"error":"forbidden"}"#.to_owned()),
            Denial::WorkspaceNotFound => (
                StatusCode::NOT_FOUND,
                r#"{"error":"workspace not found"}"#.to_owned(),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadKind(kind) => (
                StatusCode::BAD_REQUEST,
                format!(
                    "{{\"error\":{},\"kinds\":{}}}",
                    // Python `f"unknown kind {kind!r}"`: repr quotes with
                    // single quotes; Rust `{:?}` would emit doubles.
                    json_string(&format!("unknown kind '{kind}'")),
                    serde_json::to_string(&recipes::all_kinds()).expect("kinds serialize"),
                ),
            ),
            Denial::NotError(message) => (
                StatusCode::NOT_FOUND,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::ForbiddenError(message) => (
                StatusCode::FORBIDDEN,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::ValidationFailed(detail) => (
                StatusCode::BAD_REQUEST,
                ordered_error_detail("override validation failed", detail),
            ),
            Denial::RenderFailed(detail) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ordered_error_detail("render failed", detail),
            ),
            Denial::CsrfFailed(reason) => (
                StatusCode::FORBIDDEN,
                format!(
                    "{{\"detail\":{}}}",
                    json_string(&format!("CSRF Failed: {reason}"))
                ),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                crate::license::SERVER_ERROR_BODY.to_owned(),
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
            .expect("prompting denial response")
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

// ---------------------------------------------------------------------------
// CSRF (DRF SessionAuthentication.enforce_csrf)
// ---------------------------------------------------------------------------
//
// The prompting views keep DRF's default `SessionAuthentication`, whose
// `enforce_csrf` runs `CsrfViewMiddleware.process_request` +
// `process_view` for every *authenticated* unsafe request
// (`rest_framework/authentication.py:135-148`). Anonymous callers never
// reach it (401 first). This ports `django/middleware/csrf.py` (Django
// 4.2.30, matching `apps/api/requirements/base.txt`) exactly:
//
// * safe methods (`GET`/`HEAD`/`OPTIONS`/`TRACE`) skip everything;
// * an `Origin` header is verified first (same-origin or a trusted
//   origin from `CSRF_TRUSTED_ORIGINS`, which the project sets to
//   `CORS_ALLOWED_ORIGINS` — `settings/common.py:612` — so the Rust
//   `Settings.cors_allowed_origins` is the same list);
// * `is_secure()` is false: the project sets no
//   `SECURE_PROXY_SSL_HEADER` and the contract backend runs plain HTTP,
//   so the HTTPS-Referer branch is dead here exactly as there;
// * the `csrftoken` cookie carries the secret (masked or not); the
//   request token comes from the `csrfmiddlewaretoken` POST form field
//   for urlencoded POSTs, else the `X-CSRFToken` header (the only way
//   for PUT/DELETE).
//
// Intentional gaps (documented, not silent): multipart form bodies are
// not field-scanned (JSON is the API's only contract content type, so a
// multipart POST without the header reports the header as missing where
// Django would read the field); percent-decoding of the urlencoded field
// covers the token alphabet (alphanumeric tokens never encode).

/// `CSRF_SECRET_LENGTH` (`csrf.py:43`): the unmasked secret is 32 chars.
const CSRF_SECRET_LENGTH: usize = 32;
/// `CSRF_TOKEN_LENGTH` (`csrf.py:44`): a masked token is 64 chars.
const CSRF_TOKEN_LENGTH: usize = 64;
/// `CSRF_ALLOWED_CHARS` (`csrf.py:45`).
const CSRF_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// Run the CSRF gate for an authenticated unsafe request, mapping the
/// rejection reason to the DRF 403 body.
fn check_csrf(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    method: &str,
    body: &[u8],
) -> Result<(), Denial> {
    enforce_csrf(
        &state.settings().cors_allowed_origins,
        headers,
        method,
        body,
    )
    .map_err(Denial::CsrfFailed)
}

/// `_unmask_cipher_token` (`csrf.py:70-80`): decrypt the second half with
/// the first half as mask. Python's `chars[x - y]` with negative values
/// is arithmetic mod 62, reproduced here explicitly.
fn unmask_csrf_token(token: &str) -> String {
    let bytes = token.as_bytes();
    let (mask, cipher) = bytes.split_at(CSRF_SECRET_LENGTH);
    cipher
        .iter()
        .zip(mask.iter())
        .map(|(c, m)| {
            let x = alphabet_index(*c);
            let y = alphabet_index(*m);
            CSRF_ALPHABET[(x + CSRF_ALPHABET.len() - y) % CSRF_ALPHABET.len()] as char
        })
        .collect()
}

fn alphabet_index(byte: u8) -> usize {
    CSRF_ALPHABET
        .iter()
        .position(|candidate| *candidate == byte)
        .unwrap_or(0)
}

/// `_check_token_format` (`csrf.py:124-133`): length 32 or 64, alphanumerics.
fn check_csrf_format(token: &str) -> Result<(), &'static str> {
    if token.len() != CSRF_TOKEN_LENGTH && token.len() != CSRF_SECRET_LENGTH {
        return Err("has incorrect length");
    }
    if !token.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err("has invalid characters");
    }
    Ok(())
}

/// Constant-time secret comparison (`constant_time_compare`).
fn csrf_secrets_equal(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.bytes()
        .zip(right.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

/// `_does_token_match` (`csrf.py:148-163`): unmask 64-char tokens, then
/// compare against the 32-char secret.
fn csrf_token_matches(request_token: &str, secret: &str) -> bool {
    let unmasked;
    let candidate = if request_token.len() == CSRF_TOKEN_LENGTH {
        unmasked = unmask_csrf_token(request_token);
        unmasked.as_str()
    } else {
        request_token
    };
    csrf_secrets_equal(candidate, secret)
}

/// Split a `Cookie` header into `(name, value)` pairs. Values are
/// unquoted like Django's `parse_cookie` for the token alphabet (tokens
/// never contain `;`, `=`, or quotes).
fn parse_cookies(header: &str) -> Vec<(&str, &str)> {
    header
        .split(';')
        .filter_map(|pair| {
            let (name, value) = pair.split_once('=')?;
            let value = value.trim();
            let unquoted = value
                .strip_prefix('"')
                .and_then(|inner| inner.strip_suffix('"'))
                .unwrap_or(value);
            Some((name.trim(), unquoted))
        })
        .collect()
}

/// Percent-decode a urlencoded form field value (the token alphabet needs
/// no decoding, but Django unquotes before comparing).
fn percent_decode(value: &str) -> String {
    let mut out = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 3 <= bytes.len() => {
                match u8::from_str_radix(&value[index + 1..index + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The urlencoded form field for POST bodies, if present.
fn form_csrf_token(body: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?;
    for pair in text.split('&') {
        let (name, value) = pair.split_once('=')?;
        if name == "csrfmiddlewaretoken" {
            return Some(percent_decode(value));
        }
    }
    None
}

/// `is_same_domain` (`django/utils/http.py`): a `.`-prefixed pattern
/// matches the bare domain and every subdomain.
fn same_domain(host: &str, pattern: &str) -> bool {
    let host = host.to_lowercase();
    let pattern = pattern.to_lowercase();
    if let Some(suffix) = pattern.strip_prefix('.') {
        host == suffix || host.ends_with(&pattern)
    } else {
        host == pattern
    }
}

/// Split `scheme://netloc` origins into exact entries and per-scheme
/// subdomain suffixes (`csrf_trusted_origins_hosts` /
/// `allowed_origins_exact` / `allowed_origin_subdomains`).
fn split_trusted_origins(trusted: &[String]) -> (HashSet<String>, HashMap<String, Vec<String>>) {
    let mut exact = HashSet::new();
    let mut subdomains: HashMap<String, Vec<String>> = HashMap::new();
    for origin in trusted {
        if !origin.contains('*') {
            exact.insert(origin.clone());
            continue;
        }
        if let Some((scheme, rest)) = origin.split_once("://") {
            let netloc = rest.trim_start_matches('*');
            subdomains
                .entry(scheme.to_owned())
                .or_default()
                .push(netloc.to_owned());
        }
    }
    (exact, subdomains)
}

/// `_origin_verified` (`csrf.py`): same-origin, exact trusted, or
/// trusted-subdomain. `is_secure()` is false (see module docs), so the
/// good origin is always `http://{host}`.
fn origin_verified(origin: &str, host: &str, trusted: &[String]) -> bool {
    if origin == format!("http://{host}") {
        return true;
    }
    let (exact, subdomains) = split_trusted_origins(trusted);
    if exact.contains(origin) {
        return true;
    }
    let Some((scheme, netloc)) = origin.split_once("://") else {
        return false;
    };
    // A malformed netloc never verifies.
    if netloc.is_empty() || netloc.contains(char::is_whitespace) {
        return false;
    }
    subdomains
        .get(scheme)
        .map(|suffixes| suffixes.iter().any(|suffix| same_domain(netloc, suffix)))
        .unwrap_or(false)
}

/// Enforce CSRF for an authenticated unsafe request
/// (`CsrfViewMiddleware.process_view` for PUT/DELETE/POST). Returns the
/// 403 reason on rejection.
fn enforce_csrf(
    settings_trusted_origins: &[String],
    headers: &axum::http::HeaderMap,
    method: &str,
    body: &[u8],
) -> Result<(), String> {
    // Safe methods skip everything (`process_view`: GET/HEAD/OPTIONS/TRACE).
    if matches!(method, "GET" | "HEAD" | "OPTIONS" | "TRACE") {
        return Ok(());
    }
    if let Some(origin) = headers.get("origin").and_then(|value| value.to_str().ok()) {
        let host = headers
            .get("host")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        if !origin_verified(origin, host, settings_trusted_origins) {
            return Err(format!(
                "Origin checking failed - {origin} does not match any trusted origins."
            ));
        }
    }
    // `is_secure()` is false here (no `SECURE_PROXY_SSL_HEADER`), so the
    // Referer branch is dead — exactly as on the contract backend.
    let secret = csrf_secret_from_headers(headers)?;
    let (request_token, source) = csrf_request_token(headers, method, body)?;
    if !csrf_token_matches(&request_token, &secret) {
        return Err(format!("CSRF token from {source} incorrect."));
    }
    Ok(())
}

/// `_get_secret` (`csrf.py`): the `csrftoken` cookie, format-checked and
/// unmasked when masked.
fn csrf_secret_from_headers(headers: &axum::http::HeaderMap) -> Result<String, String> {
    let cookie_header = headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let secret = parse_cookies(cookie_header)
        .into_iter()
        .find(|(name, _)| *name == "csrftoken")
        .map(|(_, value)| value.to_owned());
    let Some(secret) = secret else {
        return Err("CSRF cookie not set.".to_owned());
    };
    check_csrf_format(&secret).map_err(|reason| format!("CSRF cookie {reason}."))?;
    if secret.len() == CSRF_TOKEN_LENGTH {
        Ok(unmask_csrf_token(&secret))
    } else {
        Ok(secret)
    }
}

/// The request token: the POST form field for urlencoded POSTs, else the
/// `X-CSRFToken` header. The source renders into the failure message via
/// `_bad_token_message` (`'X-Csrftoken'` is `parse_header_name` +
/// `.title()` of `HTTP_X_CSRFTOKEN`).
fn csrf_request_token(
    headers: &axum::http::HeaderMap,
    method: &str,
    body: &[u8],
) -> Result<(String, &'static str), String> {
    if method == "POST" {
        let urlencoded = headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|content_type| {
                content_type
                    .split(';')
                    .next()
                    .is_some_and(|kind| kind.trim() == "application/x-www-form-urlencoded")
            });
        if urlencoded {
            if let Some(field) = form_csrf_token(body) {
                if !field.is_empty() {
                    return csrf_token_with_source(field, "POST");
                }
            }
        }
    }
    match headers
        .get("x-csrftoken")
        .and_then(|value| value.to_str().ok())
    {
        None => Err("CSRF token missing.".to_owned()),
        Some(token) => csrf_token_with_source(token.to_owned(), "the 'X-Csrftoken' HTTP header"),
    }
}

fn csrf_token_with_source(
    token: String,
    source: &'static str,
) -> Result<(String, &'static str), String> {
    check_csrf_format(&token)
        .map(|()| (token, source))
        .map_err(|reason| format!("CSRF token from {source} {reason}."))
}

/// `{"error":…,"detail":…}` in wire order. `serde_json::json!` would sort
/// keys alphabetically (`detail` before `error`) and break byte parity;
/// insertion order is preserved by the crate's `preserve_order`.
fn ordered_error_detail(error: &str, detail: &str) -> String {
    let mut map = serde_json::Map::with_capacity(2);
    map.insert("error".to_owned(), Value::String(error.to_owned()));
    map.insert("detail".to_owned(), Value::String(detail.to_owned()));
    serde_json::to_string(&Value::Object(map)).expect("error/detail serializes")
}

/// Render an exact-JSON response body with its status.
fn json_response(status: StatusCode, body: &Value) -> Response {
    let rendered = serde_json::to_string(body).expect("prompting body serializes");
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(rendered))
        .expect("prompting response")
}

// ---------------------------------------------------------------------------
// guards
// ---------------------------------------------------------------------------

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// `request.user` or the 401. Hash-verified DRF session semantics via the
/// shared license plumbing (read-only): bad session, unknown/inactive
/// user, or hash mismatch is anonymous.
async fn actor(
    state: &AppState,
    pool: &sqlx::PgPool,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<crate::license::Actor, Denial> {
    match crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
    {
        Ok(Some(actor)) => Ok(actor),
        Ok(None) => Err(Denial::Unauthorized),
        Err(_) => Err(Denial::ServerError),
    }
}

/// One workspace row: id, slug, name.
struct Workspace {
    id: uuid::Uuid,
    slug: String,
    name: String,
}

/// `_get_workspace_or_404` (`views.py:88-92`): the default manager drops
/// soft-deleted rows.
async fn workspace_or_404(pool: &sqlx::PgPool, slug: &str) -> Result<Workspace, Denial> {
    let row: Option<(uuid::Uuid, String, String)> = sqlx::query_as(
        r#"SELECT id, slug, name FROM workspaces WHERE slug = $1 AND deleted_at IS NULL"#,
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    match row {
        Some((id, slug, name)) => Ok(Workspace { id, slug, name }),
        None => Err(Denial::WorkspaceNotFound),
    }
}

/// Superuser flag for the actor (`_is_workspace_admin/member` short-circuit,
/// `views.py:75-76,83-84`).
async fn is_superuser(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<bool, Denial> {
    let row: Option<(bool,)> = sqlx::query_as(r#"SELECT is_superuser FROM users WHERE id = $1"#)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ServerError)
}

/// `_is_workspace_admin` (`views.py:74-79`).
async fn is_workspace_admin(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    if is_superuser(pool, user_id).await? {
        return Ok(true);
    }
    let hit: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM workspace_members
           WHERE workspace_id = $1 AND member_id = $2 AND role = $3
           AND is_active AND deleted_at IS NULL"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(WORKSPACE_ADMIN_ROLE)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(hit.is_some())
}

/// `_is_workspace_member` (`views.py:82-85`).
async fn is_workspace_member(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    if is_superuser(pool, user_id).await? {
        return Ok(true);
    }
    let hit: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM workspace_members
           WHERE workspace_id = $1 AND member_id = $2 AND is_active AND deleted_at IS NULL"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(hit.is_some())
}

/// `_resolve_scope` (`views.py:95-99`): `workspace` wins, everything else
/// (including absent) is `user`.
fn resolve_scope(query: &HashMap<String, String>) -> &'static str {
    match query.get("scope").map(String::as_str) {
        Some(SCOPE_WORKSPACE) => SCOPE_WORKSPACE,
        _ => SCOPE_USER,
    }
}

/// `_check_write_permission` (`views.py:186-190`).
async fn check_write_permission(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
    scope: &str,
) -> Result<bool, Denial> {
    if scope == SCOPE_WORKSPACE {
        is_workspace_admin(pool, user_id, workspace_id).await
    } else {
        is_workspace_member(pool, user_id, workspace_id).await
    }
}

// ---------------------------------------------------------------------------
// override index + section breakdown
// ---------------------------------------------------------------------------

/// One loaded override row plus its attention flag. The composer index
/// carries resolution; `needs_attention` travels alongside keyed by the
/// same `(scope, key)` so the breakdown can attach it from the row that
/// actually resolved (`views.py:120-126,141`).
struct LoadedIndex {
    index: composer::OverrideIndex,
    // `BTreeMap`: `OverrideScope` is `Ord` but not `Hash`.
    needs_attention: BTreeMap<(composer::OverrideScope, String), bool>,
}

/// Bulk-load active overrides for the scope (`composer.py:100-122` via
/// `load_override_index`): `WHERE is_active AND workspace_id = :ws AND
/// (user_id IS NULL OR user_id = :user)`; user `None` narrows to the
/// workspace rows only.
async fn load_index(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
    user_id: Option<&uuid::Uuid>,
) -> Result<LoadedIndex, Denial> {
    let rows: Vec<(String, String, i32, Option<uuid::Uuid>, bool)> = if let Some(user) = user_id {
        sqlx::query_as(
            r#"SELECT section_key, body, version, user_id, needs_attention
               FROM prompt_section_override
               WHERE is_active AND workspace_id = $1 AND (user_id IS NULL OR user_id = $2)"#,
        )
        .bind(workspace_id)
        .bind(user)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    } else {
        sqlx::query_as(
            r#"SELECT section_key, body, version, user_id, needs_attention
               FROM prompt_section_override
               WHERE is_active AND workspace_id = $1 AND user_id IS NULL"#,
        )
        .bind(workspace_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    };
    let workspace_str = workspace_id.to_string();
    let user_str = user_id.map(uuid::Uuid::to_string);
    let composer_rows: Vec<composer::OverrideRow> = rows
        .iter()
        .map(|(key, body, version, row_user, _)| composer::OverrideRow {
            workspace_id: workspace_str.clone(),
            section_key: key.clone(),
            body: body.clone(),
            version: i64::from(*version),
            is_active: true,
            user_id: row_user.map(|id| id.to_string()),
        })
        .collect();
    let index = composer::build_override_index(
        Some(workspace_str.as_str()),
        user_str.as_deref(),
        &composer_rows,
    );
    let mut needs_attention = BTreeMap::new();
    for (scope, key) in index.keys() {
        let flag = rows
            .iter()
            .find(|(row_key, _, _, row_user, _)| {
                row_key == key
                    && match scope {
                        composer::OverrideScope::Workspace => row_user.is_none(),
                        composer::OverrideScope::User => {
                            row_user.map(|id| id.to_string()) == user_str
                        }
                    }
            })
            .map(|(_, _, _, _, flag)| *flag)
            .unwrap_or(false);
        needs_attention.insert((*scope, key.clone()), flag);
    }
    Ok(LoadedIndex {
        index,
        needs_attention,
    })
}

/// `_section_breakdown` (`views.py:102-146`): resolve every section in
/// the kind's recipe and attach `needs_attention` from the override row
/// that actually resolved, in `ResolvedSectionSerializer` field order.
fn section_breakdown(
    kind: &str,
    workspace_id: &uuid::Uuid,
    user_id: Option<&uuid::Uuid>,
    loaded: &LoadedIndex,
) -> Result<Vec<Value>, Denial> {
    let workspace_str = workspace_id.to_string();
    let user_str = user_id.map(uuid::Uuid::to_string);
    let recipe = recipes::recipe_for(kind, None).map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(recipe.len());
    for key in recipe {
        let section = registry::get_section(key).map_err(|_| Denial::ServerError)?;
        let resolved = composer::resolve_section(
            key,
            Some(workspace_str.as_str()),
            user_str.as_deref(),
            &loaded.index,
            Some(section),
        )
        .map_err(|_| Denial::ServerError)?;
        let row = if resolved.source == composer::SOURCE_WORKSPACE {
            loaded
                .needs_attention
                .get(&(composer::OverrideScope::Workspace, key.to_string()))
                .copied()
        } else if resolved.source != composer::SOURCE_DEFAULT {
            loaded
                .needs_attention
                .get(&(composer::OverrideScope::User, key.to_string()))
                .copied()
        } else {
            None
        };
        let tier = composer::effective_customizability(section, Some(workspace_str.as_str()));
        let mut entry = serde_json::Map::with_capacity(10);
        entry.insert("key".to_owned(), Value::String(resolved.key));
        entry.insert("title".to_owned(), Value::String(resolved.title));
        entry.insert("customizable".to_owned(), Value::String(tier.to_owned()));
        entry.insert("body".to_owned(), Value::String(resolved.body));
        entry.insert(
            "default_body".to_owned(),
            Value::String(section.default_body.clone()),
        );
        entry.insert("source".to_owned(), Value::String(resolved.source));
        entry.insert("version".to_owned(), Value::Number(resolved.version.into()));
        entry.insert(
            "needs_attention".to_owned(),
            Value::Bool(row.unwrap_or(false)),
        );
        entry.insert(
            "editable_at_workspace".to_owned(),
            Value::Bool(registry::tier_allows_workspace_override(tier)),
        );
        entry.insert(
            "editable_at_personal".to_owned(),
            Value::Bool(registry::tier_allows_personal_override(tier)),
        );
        out.push(Value::Object(entry));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// GET prompt-sections (list)
// ---------------------------------------------------------------------------

/// `PromptSectionListEndpoint.get` (`views.py:155-177`).
async fn list_sections(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let result = list_sections_inner(&state, &slug, &query, extension).await;
    match result {
        Ok(body) => json_response(StatusCode::OK, &body),
        Err(denial) => denial.into_response(),
    }
}

async fn list_sections_inner(
    state: &AppState,
    slug: &str,
    query: &HashMap<String, String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Value, Denial> {
    let pool = pool_of(state)?;
    let actor = actor(state, &pool, extension).await?;
    let workspace = workspace_or_404(&pool, slug).await?;
    if !is_workspace_member(&pool, &actor.id, &workspace.id).await? {
        return Err(Denial::Forbidden);
    }
    let kind = query
        .get("kind")
        .cloned()
        .unwrap_or_else(|| recipes::KIND_CODING_TASK.to_owned());
    if recipes::recipe_for(&kind, None).is_err() {
        return Err(Denial::BadKind(kind));
    }
    let scope = resolve_scope(query);
    let user = if scope == SCOPE_USER {
        Some(actor.id)
    } else {
        None
    };
    let loaded = load_index(&pool, &workspace.id, user.as_ref()).await?;
    let sections = section_breakdown(&kind, &workspace.id, user.as_ref(), &loaded)?;
    let mut body = serde_json::Map::with_capacity(3);
    body.insert("kind".to_owned(), Value::String(kind));
    body.insert("scope".to_owned(), Value::String(scope.to_owned()));
    body.insert("sections".to_owned(), Value::Array(sections));
    Ok(Value::Object(body))
}

// ---------------------------------------------------------------------------
// PUT / DELETE prompt-sections/<key>
// ---------------------------------------------------------------------------

/// Python truthiness of a parsed JSON value (`views.py:197` `or`): empty
/// string / zero / false / null / empty containers fall back to the query
/// param; anything else (including a truthy non-string) is used as-is.
fn json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(float) = number.as_f64() {
                float != 0.0
            } else {
                true
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// PUT scope (`views.py:197-198`): body `scope` when truthy, else the
/// query param; `workspace` wins, everything else is `user`. A truthy
/// non-string body scope behaves like `user` (it never equals
/// `"workspace"`, and it blocks the query fallback via `or`).
fn put_scope(data: &Value, query: &HashMap<String, String>) -> Result<&'static str, Denial> {
    let obj = data.as_object().ok_or(Denial::ServerError)?;
    match obj.get("scope") {
        Some(value) if json_truthy(value) => match value.as_str() {
            Some(SCOPE_WORKSPACE) => Ok(SCOPE_WORKSPACE),
            _ => Ok(SCOPE_USER),
        },
        _ => Ok(resolve_scope(query)),
    }
}

/// Parse a JSON body: a parse failure is DRF's `ParseError` 400; an empty
/// body reads as `{}` (form-parser behavior for the scope lookup below).
fn parse_body(raw: &[u8]) -> Result<Value, Denial> {
    if raw.is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    serde_json::from_slice(raw)
        .map_err(|err| Denial::BadDetail(format!("JSON parse error - {err}")))
}

/// One stored override row for the read serializer.
struct OverrideRecord {
    id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    user_id: Option<uuid::Uuid>,
    section_key: String,
    body: String,
    is_active: bool,
    version: i32,
    needs_attention: bool,
    updated_by_id: Option<uuid::Uuid>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

/// `PromptSectionOverrideSerializer` (`serializers.py:14-35`) in
/// `Meta.fields` order. UUID/FK keys render as strings, null FKs as
/// null; datetimes are DRF `iso-8601` `Z` strings; `is_workspace_level`
/// is the model property (`models.py:139-141`: `user_id is None`).
fn render_override(row: &OverrideRecord) -> Value {
    let mut map = serde_json::Map::with_capacity(12);
    map.insert("id".to_owned(), Value::String(row.id.to_string()));
    map.insert("workspace".to_owned(), row.workspace_id.to_string().into());
    map.insert(
        "user".to_owned(),
        row.user_id
            .map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "section_key".to_owned(),
        Value::String(row.section_key.clone()),
    );
    map.insert("body".to_owned(), Value::String(row.body.clone()));
    map.insert("is_active".to_owned(), Value::Bool(row.is_active));
    map.insert(
        "version".to_owned(),
        Value::Number(i64::from(row.version).into()),
    );
    map.insert(
        "needs_attention".to_owned(),
        Value::Bool(row.needs_attention),
    );
    map.insert(
        "is_workspace_level".to_owned(),
        Value::Bool(row.user_id.is_none()),
    );
    map.insert(
        "updated_by".to_owned(),
        row.updated_by_id
            .map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "created_at".to_owned(),
        Value::String(crate::serializer::render_datetime(&row.created_at)),
    );
    map.insert(
        "updated_at".to_owned(),
        Value::String(crate::serializer::render_datetime(&row.updated_at)),
    );
    Value::Object(map)
}

const OVERRIDE_COLUMNS: &str = "id, workspace_id, user_id, section_key, body, is_active, version, needs_attention, updated_by_id, created_at, updated_at";

/// `PromptSectionDetailEndpoint.put` (`views.py:192-243`).
async fn upsert_section(
    State(state): State<AppState>,
    Path((slug, section_key)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    let result = upsert_section_inner(
        &state,
        &slug,
        &section_key,
        &query,
        extension,
        &headers,
        &body,
    )
    .await;
    match result {
        Ok(rendered) => json_response(StatusCode::OK, &rendered),
        Err(denial) => denial.into_response(),
    }
}

async fn upsert_section_inner(
    state: &AppState,
    slug: &str,
    section_key: &str,
    query: &HashMap<String, String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: &axum::http::HeaderMap,
    raw: &[u8],
) -> Result<Value, Denial> {
    let pool = pool_of(state)?;
    let actor = actor(state, &pool, extension).await?;
    // DRF authenticates (including CSRF) before the view body runs.
    check_csrf(state, headers, "PUT", raw)?;
    let workspace = workspace_or_404(&pool, slug).await?;
    let data = parse_body(raw)?;
    let scope = put_scope(&data, query)?;
    if !check_write_permission(&pool, &actor.id, &workspace.id, scope).await? {
        return Err(Denial::Forbidden);
    }
    if registry::get_section(section_key).is_err() {
        return Err(Denial::NotError(format!("unknown section '{section_key}'")));
    }
    let section = registry::get_section(section_key).map_err(|_| Denial::ServerError)?;
    let workspace_str = workspace.id.to_string();
    let tier = composer::effective_customizability(section, Some(workspace_str.as_str()));
    if scope == SCOPE_WORKSPACE && !registry::tier_allows_workspace_override(tier) {
        return Err(Denial::ForbiddenError(format!(
            "section '{section_key}' is locked and cannot be overridden"
        )));
    }
    if scope == SCOPE_USER && !registry::tier_allows_personal_override(tier) {
        let message = if tier == registry::CUSTOMIZABLE_WORKSPACE {
            format!("section '{section_key}' cannot be personally overridden")
        } else {
            format!("section '{section_key}' is locked and cannot be overridden")
        };
        return Err(Denial::ForbiddenError(message));
    }
    let obj = data.as_object().ok_or(Denial::ServerError)?;
    let candidate = match obj.get("body") {
        None | Some(Value::Null) => {
            return Err(Denial::BadError("body is required".to_owned()));
        }
        Some(Value::String(text)) => text.clone(),
        // Python carries a non-string into `validate_override`, where
        // `len()` / the renderer raise into the generic 500 branch.
        Some(_) => return Err(Denial::ServerError),
    };
    let target_user = if scope == SCOPE_USER {
        Some(actor.id)
    } else {
        None
    };
    let loaded = load_index(&pool, &workspace.id, target_user.as_ref()).await?;
    validation::validate_override(
        section_key,
        &candidate,
        Some(workspace_str.as_str()),
        target_user.as_ref().map(uuid::Uuid::to_string).as_deref(),
        &loaded.index,
    )
    .map_err(|err| Denial::ValidationFailed(err.message().to_owned()))?;
    let row = upsert_row(
        &pool,
        &workspace.id,
        target_user.as_ref(),
        section_key,
        &candidate,
        &actor.id,
    )
    .await?;
    Ok(render_override(&row))
}

/// `_upsert` (`views.py:245-278`): update the active override in place
/// (bump version) or create one, under `SELECT … FOR UPDATE` so two
/// concurrent PUTs cannot lose an update. Only the create's unique
/// violation retries (the `IntegrityError` branch); every other failure
/// surfaces as-is, so a failed commit can never double-apply an update.
async fn upsert_row(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
    target_user: Option<&uuid::Uuid>,
    section_key: &str,
    body: &str,
    editor: &uuid::Uuid,
) -> Result<OverrideRecord, Denial> {
    match try_upsert(pool, workspace_id, target_user, section_key, body, editor).await {
        Ok(row) => Ok(row),
        Err(UpsertError::Race) => {
            // Lost the create race — an active row now exists; lock and
            // update inside a fresh transaction.
            retry_update(pool, workspace_id, target_user, section_key, body, editor).await
        }
        Err(UpsertError::Denied(denial)) => Err(denial),
    }
}

#[derive(Debug)]
enum UpsertError {
    Race,
    Denied(Denial),
}

impl From<Denial> for UpsertError {
    fn from(denial: Denial) -> Self {
        UpsertError::Denied(denial)
    }
}

async fn try_upsert(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
    target_user: Option<&uuid::Uuid>,
    section_key: &str,
    body: &str,
    editor: &uuid::Uuid,
) -> Result<OverrideRecord, UpsertError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| UpsertError::Denied(Denial::ServerError))?;
    let existing: Option<OverrideRowTuple> =
        select_for_update(&mut tx, workspace_id, target_user, section_key).await?;
    let row = match existing {
        Some(found) => apply_update(&mut tx, &record_from_row(found), body, editor).await?,
        None => {
            let id = uuid::Uuid::new_v4();
            let created: Option<OverrideRowTuple> = sqlx::query_as(&format!(
                r#"INSERT INTO prompt_section_override
                   (id, workspace_id, user_id, section_key, body, is_active, version, needs_attention, updated_by_id, created_at, updated_at)
                   VALUES ($1, $2, $3, $4, $5, true, 1, false, $6, now(), now())
                   RETURNING {OVERRIDE_COLUMNS}"#
            ))
            .bind(id)
            .bind(workspace_id)
            .bind(target_user)
            .bind(section_key)
            .bind(body)
            .bind(editor)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|err| {
                if is_unique_violation(&err) {
                    UpsertError::Race
                } else {
                    UpsertError::Denied(Denial::ServerError)
                }
            })?;
            match created {
                Some(row) => record_from_row(row),
                None => return Err(UpsertError::Denied(Denial::ServerError)),
            }
        }
    };
    tx.commit()
        .await
        .map_err(|_| UpsertError::Denied(Denial::ServerError))?;
    Ok(row)
}

async fn retry_update(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
    target_user: Option<&uuid::Uuid>,
    section_key: &str,
    body: &str,
    editor: &uuid::Uuid,
) -> Result<OverrideRecord, Denial> {
    let mut tx = pool.begin().await.map_err(|_| Denial::ServerError)?;
    let existing: Option<OverrideRowTuple> =
        select_for_update(&mut tx, workspace_id, target_user, section_key).await?;
    let Some(found) = existing else {
        return Err(Denial::ServerError);
    };
    let row = record_from_row(found);
    let updated = apply_update(&mut tx, &row, body, editor).await?;
    tx.commit().await.map_err(|_| Denial::ServerError)?;
    Ok(updated)
}

type OverrideRowTuple = (
    uuid::Uuid,
    uuid::Uuid,
    Option<uuid::Uuid>,
    String,
    String,
    bool,
    i32,
    bool,
    Option<uuid::Uuid>,
    chrono::DateTime<chrono::Utc>,
    chrono::DateTime<chrono::Utc>,
);

fn record_from_row(row: OverrideRowTuple) -> OverrideRecord {
    OverrideRecord {
        id: row.0,
        workspace_id: row.1,
        user_id: row.2,
        section_key: row.3,
        body: row.4,
        is_active: row.5,
        version: row.6,
        needs_attention: row.7,
        updated_by_id: row.8,
        created_at: row.9,
        updated_at: row.10,
    }
}

async fn select_for_update(
    tx: &mut sqlx::PgConnection,
    workspace_id: &uuid::Uuid,
    target_user: Option<&uuid::Uuid>,
    section_key: &str,
) -> Result<Option<OverrideRowTuple>, Denial> {
    if target_user.is_some() {
        sqlx::query_as(&format!(
            r#"SELECT {OVERRIDE_COLUMNS} FROM prompt_section_override
               WHERE workspace_id = $1 AND section_key = $2 AND is_active AND user_id = $3
               FOR UPDATE"#
        ))
        .bind(workspace_id)
        .bind(section_key)
        .bind(target_user)
        .fetch_optional(tx)
        .await
        .map_err(|_| Denial::ServerError)
    } else {
        sqlx::query_as(&format!(
            r#"SELECT {OVERRIDE_COLUMNS} FROM prompt_section_override
               WHERE workspace_id = $1 AND section_key = $2 AND is_active AND user_id IS NULL
               FOR UPDATE"#
        ))
        .bind(workspace_id)
        .bind(section_key)
        .fetch_optional(tx)
        .await
        .map_err(|_| Denial::ServerError)
    }
}

/// `_apply_update` (`views.py:280-290`): new body, `version + 1` (no
/// read-modify-write drift), attention cleared, editor stamped.
async fn apply_update(
    tx: &mut sqlx::PgConnection,
    row: &OverrideRecord,
    body: &str,
    editor: &uuid::Uuid,
) -> Result<OverrideRecord, Denial> {
    let updated: Option<OverrideRowTuple> = sqlx::query_as(&format!(
        r#"UPDATE prompt_section_override
           SET body = $1, version = version + 1, needs_attention = false,
               updated_by_id = $2, updated_at = now()
           WHERE id = $3
           RETURNING {OVERRIDE_COLUMNS}"#
    ))
    .bind(body)
    .bind(editor)
    .bind(row.id)
    .fetch_optional(tx)
    .await
    .map_err(|_| Denial::ServerError)?;
    updated.map(record_from_row).ok_or(Denial::ServerError)
}

/// SQLSTATE 23505 carries the lost-create race (the `IntegrityError`
/// branch); anything else on the create path is a generic failure.
fn is_unique_violation(err: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db) = err {
        return db.code().as_deref() == Some("23505");
    }
    false
}

/// `PromptSectionDetailEndpoint.delete` (`views.py:292-319`):
/// soft-deactivate the active row at this scope, 204 with an empty body.
async fn delete_override(
    State(state): State<AppState>,
    Path((slug, section_key)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: axum::http::HeaderMap,
) -> Response {
    let result =
        delete_override_inner(&state, &slug, &section_key, &query, extension, &headers).await;
    match result {
        Ok(()) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(axum::body::Body::empty())
            .expect("delete 204 response"),
        Err(denial) => denial.into_response(),
    }
}

async fn delete_override_inner(
    state: &AppState,
    slug: &str,
    section_key: &str,
    query: &HashMap<String, String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: &axum::http::HeaderMap,
) -> Result<(), Denial> {
    let pool = pool_of(state)?;
    let actor = actor(state, &pool, extension).await?;
    check_csrf(state, headers, "DELETE", &[])?;
    let workspace = workspace_or_404(&pool, slug).await?;
    let scope = resolve_scope(query);
    if !check_write_permission(&pool, &actor.id, &workspace.id, scope).await? {
        return Err(Denial::Forbidden);
    }
    if registry::get_section(section_key).is_err() {
        return Err(Denial::NotError(format!("unknown section '{section_key}'")));
    }
    let target_user = if scope == SCOPE_USER {
        Some(actor.id)
    } else {
        None
    };
    let row: Option<(uuid::Uuid,)> = if target_user.is_some() {
        sqlx::query_as(
            r#"SELECT id FROM prompt_section_override
               WHERE workspace_id = $1 AND section_key = $2 AND is_active AND user_id = $3"#,
        )
        .bind(workspace.id)
        .bind(section_key)
        .bind(target_user)
        .fetch_optional(&pool)
        .await
        .map_err(|_| Denial::ServerError)?
    } else {
        sqlx::query_as(
            r#"SELECT id FROM prompt_section_override
               WHERE workspace_id = $1 AND section_key = $2 AND is_active AND user_id IS NULL"#,
        )
        .bind(workspace.id)
        .bind(section_key)
        .fetch_optional(&pool)
        .await
        .map_err(|_| Denial::ServerError)?
    };
    let Some((id,)) = row else {
        return Err(Denial::NotError(
            "no active override at this scope".to_owned(),
        ));
    };
    sqlx::query(
        r#"UPDATE prompt_section_override
           SET is_active = false, updated_by_id = $1, updated_at = now()
           WHERE id = $2"#,
    )
    .bind(actor.id)
    .bind(id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// GET prompts/<kind>/compiled
// ---------------------------------------------------------------------------

/// `PromptCompiledEndpoint.get` (`views.py:328-357`).
async fn compiled_template(
    State(state): State<AppState>,
    Path((slug, kind)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let result = compiled_template_inner(&state, &slug, &kind, &query, extension).await;
    match result {
        Ok(body) => json_response(StatusCode::OK, &body),
        Err(denial) => denial.into_response(),
    }
}

async fn compiled_template_inner(
    state: &AppState,
    slug: &str,
    kind: &str,
    query: &HashMap<String, String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Value, Denial> {
    let pool = pool_of(state)?;
    let actor = actor(state, &pool, extension).await?;
    let workspace = workspace_or_404(&pool, slug).await?;
    if !is_workspace_member(&pool, &actor.id, &workspace.id).await? {
        return Err(Denial::Forbidden);
    }
    if recipes::recipe_for(kind, None).is_err() {
        return Err(Denial::BadKind(kind.to_owned()));
    }
    let scope = resolve_scope(query);
    let user = if scope == SCOPE_USER {
        Some(actor.id)
    } else {
        None
    };
    let loaded = load_index(&pool, &workspace.id, user.as_ref()).await?;
    let workspace_str = workspace.id.to_string();
    let user_str = user.as_ref().map(uuid::Uuid::to_string);
    let compiled = composer::compile_template(
        kind,
        Some(workspace_str.as_str()),
        user_str.as_deref(),
        &loaded.index,
    )
    .map_err(|_| Denial::ServerError)?;
    let mut body = serde_json::Map::with_capacity(4);
    body.insert("kind".to_owned(), Value::String(kind.to_owned()));
    body.insert("scope".to_owned(), Value::String(scope.to_owned()));
    body.insert(
        "template_body".to_owned(),
        Value::String(compiled.template_body),
    );
    // Dual compilation (§9.1, `views.py:351-356`): when resolving for a
    // user who has overrides, also surface the workspace-only template
    // that automatic runs would use.
    if user.is_some()
        && compiled
            .resolved
            .iter()
            .any(|section| section.source.starts_with("user:"))
    {
        let automatic =
            composer::compile_template(kind, Some(workspace_str.as_str()), None, &loaded.index)
                .map_err(|_| Denial::ServerError)?;
        body.insert(
            "automatic_template_body".to_owned(),
            Value::String(automatic.template_body),
        );
    }
    Ok(Value::Object(body))
}

// ---------------------------------------------------------------------------
// POST prompts/<kind>/preview
// ---------------------------------------------------------------------------

/// `PromptPreviewEndpoint.post` (`views.py:366-439`).
async fn preview_prompt(
    State(state): State<AppState>,
    Path((slug, kind)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    let result = preview_prompt_inner(&state, &slug, &kind, extension, &headers, &body).await;
    match result {
        Ok(rendered) => json_response(StatusCode::OK, &rendered),
        Err(denial) => denial.into_response(),
    }
}

async fn preview_prompt_inner(
    state: &AppState,
    slug: &str,
    kind: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: &axum::http::HeaderMap,
    raw: &[u8],
) -> Result<Value, Denial> {
    let pool = pool_of(state)?;
    let actor = actor(state, &pool, extension).await?;
    check_csrf(state, headers, "POST", raw)?;
    let workspace = workspace_or_404(&pool, slug).await?;
    let admin = is_workspace_admin(&pool, &actor.id, &workspace.id).await?;
    if !admin && !is_workspace_member(&pool, &actor.id, &workspace.id).await? {
        return Err(Denial::Forbidden);
    }
    if recipes::recipe_for(kind, None).is_err() {
        return Err(Denial::BadKind(kind.to_owned()));
    }
    let data = parse_body(raw)?;
    // Members may preview their own (user-scope) composition; the
    // workspace default stays admin-only (`views.py:379-384`). Only the
    // exact `"user"` string selects the user scope — a missing, null, or
    // otherwise-valued scope is the workspace default.
    let scope_user = matches!(
        data.as_object().and_then(|obj| obj.get("scope")),
        Some(Value::String(scope)) if scope == SCOPE_USER
    );
    if !admin && !scope_user {
        return Err(Denial::Forbidden);
    }
    let user = if scope_user { Some(actor.id) } else { None };

    let context_value = if kind == recipes::KIND_SCHEDULER {
        load_scheduler_context(&pool, &workspace, &data).await?
    } else {
        load_issue_context(&pool, &workspace, kind, &data).await?
    };

    // Optional unsaved draft (`views.py:393-423`): render this section's
    // draft body in place of its resolved one.
    let mut draft_overrides: Option<HashMap<String, String>> = None;
    if let Some(section_key) = data
        .as_object()
        .and_then(|obj| obj.get("section_key"))
        .and_then(Value::as_str)
    {
        let draft_body = data
            .as_object()
            .and_then(|obj| obj.get("body"))
            .and_then(Value::as_str)
            .ok_or_else(|| Denial::BadError("body is required to preview a draft".to_owned()))?;
        if recipes::recipe_for(kind, None)
            .map(|recipe| !recipe.contains(&section_key))
            .unwrap_or(true)
        {
            return Err(Denial::BadError(format!(
                "section '{section_key}' is not part of the '{kind}' prompt"
            )));
        }
        let section = registry::get_section(section_key).map_err(|_| Denial::ServerError)?;
        let workspace_str = workspace.id.to_string();
        let tier = composer::effective_customizability(section, Some(workspace_str.as_str()));
        let allowed = if scope_user {
            registry::tier_allows_personal_override(tier)
        } else {
            registry::tier_allows_workspace_override(tier)
        };
        if !allowed {
            let scope_name = if scope_user {
                SCOPE_USER
            } else {
                SCOPE_WORKSPACE
            };
            return Err(Denial::ForbiddenError(format!(
                "section '{section_key}' cannot be overridden at scope '{scope_name}'"
            )));
        }
        draft_overrides = Some(HashMap::from([(
            section_key.to_owned(),
            draft_body.to_owned(),
        )]));
    }

    let workspace_str = workspace.id.to_string();
    let user_str = user.as_ref().map(uuid::Uuid::to_string);
    let loaded = load_index(&pool, &workspace.id, user.as_ref()).await?;
    let composed = composer::compose(
        kind,
        Some(workspace_str.as_str()),
        user_str.as_deref(),
        &loaded.index,
        &context_value,
        draft_overrides.as_ref(),
        // The preview stub run carries no executor selection
        // (`getattr(run, "executor_kind", "local_runner")`).
        None,
    )
    .map_err(|err| Denial::RenderFailed(err.message().to_owned()))?;
    let mut body = serde_json::Map::with_capacity(2);
    body.insert("kind".to_owned(), Value::String(kind.to_owned()));
    body.insert("prompt".to_owned(), Value::String(composed.text));
    Ok(Value::Object(body))
}

/// A required UUID request id (`issue_id` / `binding_id`): missing, null,
/// or otherwise-falsy values are the 400; a present-but-unparseable value
/// flows into the lookup, where Django's `ValidationError` is the 404
/// (`views.py:442-455,463-481`).
fn required_id(data: &Value, key: &str, missing_error: &str) -> Result<uuid::Uuid, Denial> {
    let value = data.as_object().and_then(|obj| obj.get(key));
    match value {
        None | Some(Value::Null) => Err(Denial::BadError(missing_error.to_owned())),
        Some(candidate) if !json_truthy(candidate) => {
            Err(Denial::BadError(missing_error.to_owned()))
        }
        Some(Value::String(text)) => text
            .parse::<uuid::Uuid>()
            .map_err(|_| Denial::NotError(not_found_for(key))),
        Some(other) => other
            .to_string()
            .parse::<uuid::Uuid>()
            .map_err(|_| Denial::NotError(not_found_for(key))),
    }
}

fn not_found_for(key: &str) -> String {
    if key == "binding_id" {
        "scheduler binding not found".to_owned()
    } else {
        "issue not found".to_owned()
    }
}

// ---------------------------------------------------------------------------
// preview contexts
// ---------------------------------------------------------------------------

/// One issue row for the context builders.
struct IssueRow {
    id: uuid::Uuid,
    project_id: uuid::Uuid,
    state_id: Option<uuid::Uuid>,
    parent_id: Option<uuid::Uuid>,
    name: Option<String>,
    description_stripped: Option<String>,
    priority: Option<String>,
    sequence_id: i32,
    target_date: Option<chrono::NaiveDate>,
    git_work_branch: Option<String>,
    workpad: Option<String>,
}

/// Row shapes for the preview loaders. Tuple aliases keep the
/// `query_as` call sites under the `type_complexity` lint without hiding
/// the column order the SQL selects.
type IssueCols = (
    uuid::Uuid,
    uuid::Uuid,
    Option<uuid::Uuid>,
    Option<uuid::Uuid>,
    Option<String>,
    Option<String>,
    Option<String>,
    i32,
    Option<chrono::NaiveDate>,
    Option<String>,
    Option<String>,
);
type ProjectCols = (
    uuid::Uuid,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
);
type CommentCols = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<uuid::Uuid>,
    Option<chrono::DateTime<chrono::Utc>>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<bool>,
);
type ChildCols = (uuid::Uuid, Option<String>, i32, String, Option<String>);
type TargetCols = (
    uuid::Uuid,
    Option<String>,
    i32,
    String,
    Option<String>,
    Option<String>,
);
type ReviewCols = (String, Option<String>, String, bool, bool, String, String);
type BindingCols = (
    uuid::Uuid,
    Option<uuid::Uuid>,
    uuid::Uuid,
    Option<String>,
    Option<String>,
);
type SchedulerCols = (String, String, Option<String>, Option<String>);
type ParentCols = (Option<String>, Option<uuid::Uuid>, Option<String>, i64);

/// `_issue_context` (`views.py:441-461`): the issue must live in this
/// workspace (tenant isolation); anything unresolvable is the 404. The
/// default manager drops soft-deleted issues.
async fn load_issue_context(
    pool: &sqlx::PgPool,
    workspace: &Workspace,
    kind: &str,
    data: &Value,
) -> Result<Value, Denial> {
    let issue_id = required_id(data, "issue_id", "issue_id is required for this kind")?;
    let row: Option<IssueCols> = sqlx::query_as(
        r#"SELECT i.id, i.project_id, i.state_id, i.parent_id, i.name, i.description_stripped,
                  i.priority, i.sequence_id, i.target_date, i.git_work_branch, i.workpad
           FROM issues i
           WHERE i.id = $1 AND i.workspace_id = $2 AND i.deleted_at IS NULL"#,
    )
    .bind(issue_id)
    .bind(workspace.id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some(cols) = row else {
        return Err(Denial::NotError("issue not found".to_owned()));
    };
    let issue = IssueRow {
        id: cols.0,
        project_id: cols.1,
        state_id: cols.2,
        parent_id: cols.3,
        name: cols.4,
        description_stripped: cols.5,
        priority: cols.6,
        sequence_id: cols.7,
        target_date: cols.8,
        git_work_branch: cols.9,
        workpad: cols.10,
    };

    let project = load_project(pool, &issue.project_id).await?;
    let state = match issue.state_id {
        Some(id) => load_state(pool, &id).await?,
        None => None,
    };
    let labels = load_labels(pool, &issue.id).await?;
    let assignees = load_assignees(pool, &issue.id).await?;
    let project_states = load_project_states(pool, &issue.project_id).await?;
    let children = load_children(pool, &issue.id).await?;
    let (relation_rows, refs, directional_refs) = load_relations(pool, &issue.id).await?;
    let comments = load_comments(pool, &issue.id).await?;
    let reviews = load_code_reviews(pool, &issue.id).await?;
    let repo = load_repo(pool, &project, issue.git_work_branch.as_deref()).await?;
    let ancestors = load_ancestors(pool, &issue).await?;
    let prior_runs: i64 =
        sqlx::query_as(r#"SELECT COUNT(*) FROM agent_run WHERE work_item_id = $1"#)
            .bind(issue.id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?
            .map(|row: (i64,)| row.0)
            .unwrap_or(0);
    let tick = load_tick(pool, &project, &state, &issue.id).await?;
    let parent_done_payload = load_parent_payload(pool, &issue.id).await?;

    let related = context::related_context(&issue.id.to_string(), &relation_rows, &refs);
    let by_type = context::directional_relations_context(
        &issue.id.to_string(),
        &relation_rows,
        &directional_refs,
    );
    let relations = context::relations_context(&by_type);

    let (parent, lineage) = parent_and_lineage(pool, &ancestors).await?;

    let input = context::IssueContextInput {
        issue_id: issue.id.to_string(),
        project_identifier: project.identifier.clone(),
        sequence_id: i64::from(issue.sequence_id),
        title: issue.name.clone(),
        description_stripped: issue.description_stripped.clone(),
        state_name: state.as_ref().map(|s| s.name.clone()),
        state_group: state.as_ref().map(|s| s.group.clone()),
        priority: issue.priority.clone(),
        labels,
        assignees,
        target_date_iso: issue.target_date.map(|d| d.format("%Y-%m-%d").to_string()),
        project_states,
        workspace_slug: workspace.slug.clone(),
        workspace_name: workspace.name.clone(),
        project_id: project.id.clone(),
        project_name: project.name.clone(),
        project_description: project.description.clone(),
        repo,
        code_reviews: context::code_reviews_context(&reviews),
        parent,
        ancestors: lineage,
        children: Value::Array(context::children_context(&children)),
        related: Value::Array(related),
        relations,
        // `_FakeRun` (`views.py:65-72`): a fresh id with no trigger,
        // `local_runner` executor, and an empty tool plan.
        run_id: uuid::Uuid::new_v4().to_string(),
        run_template_name: template_name_for(state.as_ref()),
        attempt: context::compute_attempt(prior_runs),
        trigger: None,
        executor_kind: "local_runner".to_owned(),
        available_tools: Value::Null,
        unavailable_capabilities: Value::Null,
        extra_toolsets_enabled: false,
        extra_toolsets_schema_tool: String::new(),
        limits: Value::Null,
        tick,
        comments_section: context::comments_section(&comments),
        parent_done_payload,
        workpad_body: issue.workpad.clone().unwrap_or_default(),
    };
    let mut context_value = context::build_context(&input);
    // Honor the requested kind even if it differs from the issue's
    // state-derived kind (`views.py:457-460`).
    if let Some(run) = context_value.get_mut("run") {
        if let Some(kind_value) = run.get_mut("kind") {
            *kind_value = Value::String(kind.to_owned());
        }
    }
    Ok(context_value)
}

/// `template_name_for` (`orchestration/agent_phases.py:179-188`):
/// the registered ticking state's template, else the coding default.
fn template_name_for(state: Option<&StateRow>) -> String {
    match state {
        Some(s) if s.group == "started" && s.name == "In Progress" => {
            recipes::KIND_CODING_TASK.to_owned()
        }
        Some(s) if s.group == "review" && s.name == "In Review" => "review".to_owned(),
        Some(s) if s.group == "test" && s.name == "In Test" => "test".to_owned(),
        _ => recipes::KIND_CODING_TASK.to_owned(),
    }
}

/// One project row for the context builders.
struct ProjectRow {
    id: String,
    identifier: String,
    name: String,
    description: Option<String>,
    repo_url: Option<String>,
    base_branch: Option<String>,
    pool: i64,
    interval_impl: i64,
    interval_review: i64,
    interval_test: i64,
}

async fn load_project(pool: &sqlx::PgPool, project_id: &uuid::Uuid) -> Result<ProjectRow, Denial> {
    let row: Option<ProjectCols> = sqlx::query_as(
        r#"SELECT id, identifier, name, description, repo_url, base_branch,
                  agent_default_max_ticks, agent_default_interval_seconds,
                  agent_review_default_interval_seconds, agent_test_default_interval_seconds
           FROM projects WHERE id = $1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // The issue row's FK guarantees the project exists; a missing row is
    // an integrity failure, never a user-facing 404.
    let row = row.ok_or(Denial::ServerError)?;
    Ok(ProjectRow {
        id: row.0.to_string(),
        identifier: row.1,
        name: row.2,
        description: row.3,
        repo_url: row.4.filter(|s| !s.is_empty()),
        base_branch: row.5.filter(|s| !s.is_empty()),
        pool: row.6.map(i64::from).unwrap_or(10),
        interval_impl: row.7.map(i64::from).unwrap_or(10800),
        interval_review: row.8.map(i64::from).unwrap_or(10800),
        interval_test: row.9.map(i64::from).unwrap_or(10800),
    })
}

/// One state row for the context builders.
struct StateRow {
    name: String,
    group: String,
}

async fn load_state(
    pool: &sqlx::PgPool,
    state_id: &uuid::Uuid,
) -> Result<Option<StateRow>, Denial> {
    let row: Option<(String, String)> =
        sqlx::query_as(r#"SELECT name, "group" FROM states WHERE id = $1"#)
            .bind(state_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|(name, group)| StateRow { name, group }))
}

/// `issue.labels` (`context.py:536`): label names, newest first
/// (`Label.Meta.ordering`), through + target soft-delete filtered.
async fn load_labels(pool: &sqlx::PgPool, issue_id: &uuid::Uuid) -> Result<Vec<String>, Denial> {
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"SELECT l.name FROM issue_labels il
           JOIN labels l ON l.id = il.label_id
           WHERE il.issue_id = $1 AND il.deleted_at IS NULL AND l.deleted_at IS NULL
           ORDER BY l.created_at DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(rows.into_iter().map(|row| row.0).collect())
}

/// `issue.assignees` (`context.py:537`): display name, else email, else
/// empty (`context.py:257-265`).
async fn load_assignees(pool: &sqlx::PgPool, issue_id: &uuid::Uuid) -> Result<Vec<String>, Denial> {
    let rows: Vec<(Option<String>, Option<String>)> = sqlx::query_as(
        r#"SELECT u.display_name, u.email FROM issue_assignees ia
           JOIN users u ON u.id = ia.assignee_id
           WHERE ia.issue_id = $1 AND ia.deleted_at IS NULL
           ORDER BY u.id"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(|(display, email)| {
            display
                .filter(|s| !s.is_empty())
                .or_else(|| email.filter(|s| !s.is_empty()))
                .unwrap_or_default()
        })
        .collect())
}

/// `State.objects.filter(project=…)` (`context.py:538-544`): the state
/// manager excludes triage; `Meta.ordering` is `sequence`.
async fn load_project_states(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
) -> Result<Vec<context::ProjectStateView>, Denial> {
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        r#"SELECT name, "group", description FROM states
           WHERE project_id = $1 AND deleted_at IS NULL AND "group" != 'triage'
           ORDER BY sequence"#,
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(|(name, group, description)| context::ProjectStateView {
            name,
            group,
            description,
        })
        .collect())
}

/// `_children_context` (`context.py:94-107`): direct children through
/// `issue_objects` (triage / archived / draft excluded), oldest first.
/// The `!= 'triage'` comparison drops NULL-state rows with it, matching
/// Django's `NOT (group = 'triage')` three-valued logic.
async fn load_children(
    pool: &sqlx::PgPool,
    issue_id: &uuid::Uuid,
) -> Result<Vec<context::IssueRef>, Denial> {
    let rows: Vec<ChildCols> = sqlx::query_as(
        r#"SELECT i.id, i.name, i.sequence_id, p.identifier, s.name
           FROM issues i
           JOIN projects p ON p.id = i.project_id
           LEFT JOIN states s ON s.id = i.state_id
           WHERE i.parent_id = $1 AND i.deleted_at IS NULL
             AND s."group" != 'triage'
             AND i.archived_at IS NULL
             AND p.archived_at IS NULL
             AND NOT i.is_draft
           ORDER BY i.created_at ASC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(
            |(_, name, sequence, project_identifier, state)| context::IssueRef {
                identifier: context::issue_identifier(&project_identifier, i64::from(sequence)),
                title: name.unwrap_or_default(),
                state: state.unwrap_or_default(),
            },
        )
        .collect())
}

/// Relation rows plus the other-end targets for both the symmetric
/// (`_related_context`) and directional
/// (`_directional_relations_context`) builders: the both-endpoints query
/// shape of `app/views/issue/relation.py`, newest first, soft-deleted
/// relation rows dropped by the default manager. Targets are live
/// (non-deleted) issues; each carries its *own* project's identifier
/// (`context.py:35-43`).
async fn load_relations(
    pool: &sqlx::PgPool,
    issue_id: &uuid::Uuid,
) -> Result<
    (
        Vec<context::RelationRow>,
        HashMap<String, context::IssueRef>,
        HashMap<String, context::DirectionalRef>,
    ),
    Denial,
> {
    let rows: Vec<(uuid::Uuid, uuid::Uuid, String)> = sqlx::query_as(
        r#"SELECT issue_id, related_issue_id, relation_type FROM issue_relations
           WHERE (issue_id = $1 OR related_issue_id = $1) AND deleted_at IS NULL
           ORDER BY created_at DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let mut other_ids: Vec<uuid::Uuid> = Vec::new();
    let mut seen_ids = HashSet::new();
    for (left, right, _) in &rows {
        let other = if left == issue_id { *right } else { *left };
        if other != *issue_id && seen_ids.insert(other) {
            other_ids.push(other);
        }
    }
    let targets: Vec<TargetCols> = if other_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as(
            r#"SELECT i.id, i.name, i.sequence_id, p.identifier, s.name, s."group"
               FROM issues i
               JOIN projects p ON p.id = i.project_id
               LEFT JOIN states s ON s.id = i.state_id
               WHERE i.id = ANY($1) AND i.deleted_at IS NULL"#,
        )
        .bind(&other_ids)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    };
    let mut refs = HashMap::new();
    let mut directional_refs = HashMap::new();
    for (id, name, sequence, project_identifier, state, group) in targets {
        let key = id.to_string();
        let title = name.unwrap_or_default();
        let state_name = state.unwrap_or_default();
        refs.insert(
            key.clone(),
            context::IssueRef {
                identifier: context::issue_identifier(&project_identifier, i64::from(sequence)),
                title: title.clone(),
                state: state_name.clone(),
            },
        );
        directional_refs.insert(
            key,
            context::DirectionalRef {
                identifier: context::issue_identifier(&project_identifier, i64::from(sequence)),
                title,
                state: state_name,
                state_group: group.unwrap_or_default(),
            },
        );
    }
    let relation_rows = rows
        .into_iter()
        .map(|(left, right, relation_type)| context::RelationRow {
            issue_id: left.to_string(),
            related_issue_id: right.to_string(),
            relation_type,
        })
        .collect();
    Ok((relation_rows, refs, directional_refs))
}

/// `_comments_section` (`context.py:297-329`): unfolded comments as a
/// numbered chronological log, `fold`-labeled rows excluded, author
/// labels resolved. `created_at` renders via `isoformat` (`+00:00`,
/// microseconds omitted when zero) — not the DRF `Z` form.
async fn load_comments(
    pool: &sqlx::PgPool,
    issue_id: &uuid::Uuid,
) -> Result<Vec<context::CommentView>, Denial> {
    let rows: Vec<CommentCols> = sqlx::query_as(
        r#"SELECT c.comment_stripped, c.speaker_type, c.speaker_label, c.speaker_agent_run_id,
                  c.created_at, u.display_name, u.email, u.username, u.is_bot
           FROM issue_comments c
           LEFT JOIN users u ON u.id = c.actor_id
           WHERE c.issue_id = $1 AND c.deleted_at IS NULL
             AND NOT (c.labels @> ARRAY['fold']::varchar[])
           ORDER BY c.created_at ASC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(
            |(
                stripped,
                speaker_type,
                speaker_label,
                run_id,
                created_at,
                display,
                email,
                username,
                is_bot,
            )| {
                let actor = match (&display, &email, &username, &is_bot) {
                    (None, None, None, None) => None,
                    _ => Some(context::ActorView {
                        display_name: display,
                        email,
                        username,
                        is_bot: is_bot.unwrap_or(false),
                    }),
                };
                context::CommentView {
                    body: stripped.unwrap_or_default(),
                    speaker_type: speaker_type.unwrap_or_default(),
                    speaker_label,
                    actor,
                    created_at_iso: created_at.map(render_isoformat),
                    run_id: run_id.map(|id| id.to_string()),
                }
            },
        )
        .collect())
}

/// Python `datetime.isoformat()` for an aware UTC timestamp: `+00:00`
/// offset, microseconds only when nonzero (`context.py:322`).
fn render_isoformat(dt: chrono::DateTime<chrono::Utc>) -> String {
    // `AutoSi` trims trailing zeros (`.123000` -> `.123`); Python always
    // prints six digits when microseconds are nonzero.
    if dt.timestamp_subsec_micros() == 0 {
        dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
    } else {
        dt.to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
    }
}

/// `_code_reviews_context` (`context.py:472-494`): attached review links,
/// newest first (`Meta.ordering`).
async fn load_code_reviews(
    pool: &sqlx::PgPool,
    issue_id: &uuid::Uuid,
) -> Result<Vec<context::CodeReviewView>, Denial> {
    let rows: Vec<ReviewCols> = sqlx::query_as(
        r#"SELECT url, title, state, merged, draft, provider, external_iid
           FROM git_code_review_links
           WHERE issue_id = $1 AND deleted_at IS NULL
           ORDER BY created_at DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(
            |(url, title, state, merged, draft, provider, external_iid)| context::CodeReviewView {
                url,
                title,
                state,
                merged,
                draft,
                provider,
                external_iid,
            },
        )
        .collect())
}

/// `_repo_context` (`context.py:434-469`): provider-neutral repo view plus
/// the first bound remote with its adapter names. `get_adapter` raises
/// `KeyError` for unknown providers — the `None` branch renders
/// `provider.title()` + `"code review"`.
async fn load_repo(
    pool: &sqlx::PgPool,
    project: &ProjectRow,
    work_branch: Option<&str>,
) -> Result<Value, Denial> {
    let project_view = context::ProjectRepoView {
        url: project.repo_url.clone(),
        base_branch: project.base_branch.clone(),
    };
    let binding: Option<(String, String, String)> = sqlx::query_as(
        r#"SELECT r.provider, r.host_url, r.full_name
           FROM git_repository_bindings b
           JOIN git_repositories r ON r.id = b.repository_id
           WHERE b.project_id = $1 AND b.deleted_at IS NULL AND r.deleted_at IS NULL
           ORDER BY b.created_at ASC
           LIMIT 1"#,
    )
    .bind(
        project
            .id
            .parse::<uuid::Uuid>()
            .map_err(|_| Denial::ServerError)?,
    )
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (remote, adapter) = match binding {
        None => (None, None),
        Some((provider, host_url, full_name)) => {
            let adapter = match provider.to_lowercase().as_str() {
                "github" => Some(context::AdapterNames {
                    display_name: "GitHub".to_owned(),
                    code_review_term: "pull request".to_owned(),
                }),
                "gitlab" => Some(context::AdapterNames {
                    display_name: "GitLab".to_owned(),
                    code_review_term: "merge request".to_owned(),
                }),
                // Unknown provider: the `KeyError` branch (`None`
                // renders `provider.title()` in `repo_context`).
                _ => None,
            };
            (
                Some(context::RemoteView {
                    provider,
                    host_url,
                    full_name,
                }),
                adapter,
            )
        }
    };
    Ok(context::repo_context(
        &project_view,
        work_branch,
        remote.as_ref(),
        adapter.as_ref(),
    ))
}

/// One ancestor-chain node: the issue plus its parent link.
struct AncestorNode {
    id: uuid::Uuid,
    project_identifier: String,
    title: String,
}

/// `_ancestor_chain` (`context.py:55-75`): `[issue, parent, … root]`
/// with the visited-id cycle guard and the depth-50 cap. Plain FK
/// follows — no soft-delete filtering, exactly like the ORM walk.
async fn load_ancestors(
    pool: &sqlx::PgPool,
    issue: &IssueRow,
) -> Result<Vec<AncestorNode>, Denial> {
    let project = load_project(pool, &issue.project_id).await?;
    let mut chain = vec![AncestorNode {
        id: issue.id,
        project_identifier: project.identifier,
        title: issue.name.clone().unwrap_or_default(),
    }];
    let mut seen = HashSet::from([issue.id]);
    let mut next = issue.parent_id;
    while let Some(id) = next {
        if chain.len() >= 50 || !seen.insert(id) {
            break;
        }
        let row: Option<(Option<String>, Option<uuid::Uuid>, Option<uuid::Uuid>)> =
            sqlx::query_as(r#"SELECT name, project_id, parent_id FROM issues WHERE id = $1"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(|_| Denial::ServerError)?;
        let Some((name, project_id, parent_id)) = row else {
            break;
        };
        let project_identifier = match project_id {
            Some(pid) => load_project(pool, &pid).await?.identifier,
            None => String::new(),
        };
        chain.push(AncestorNode {
            id,
            project_identifier,
            title: name.unwrap_or_default(),
        });
        next = parent_id;
    }
    Ok(chain)
}

/// The `parent` block plus the `lineage` trail (`context.py:582-604`):
/// the direct parent inlined with its comment count; the full trail only
/// past a grandparent (`len(ancestors) > 2`), else null. Identifiers use
/// each node's *own* project (`context.py:35-43`).
async fn parent_and_lineage(
    pool: &sqlx::PgPool,
    ancestors: &[AncestorNode],
) -> Result<(Option<context::ParentView>, Vec<context::LineageNode>), Denial> {
    let parent = match ancestors.get(1) {
        None => None,
        Some(node) => {
            let row: Option<ParentCols> = sqlx::query_as(
                r#"SELECT i.name, i.state_id, i.git_work_branch,
                              (SELECT COUNT(*) FROM issue_comments c
                               WHERE c.issue_id = i.id AND c.deleted_at IS NULL)
                       FROM issues i WHERE i.id = $1"#,
            )
            .bind(node.id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            match row {
                None => None,
                Some((name, state_id, work_branch, comments_count)) => {
                    let state_name = match state_id {
                        Some(id) => load_state(pool, &id).await?.map(|s| s.name),
                        None => None,
                    };
                    // The parent's description rides the block; reload it:
                    // the chain walk carries identifiers + titles only.
                    let description: Option<(Option<String>,)> =
                        sqlx::query_as(r#"SELECT description_stripped FROM issues WHERE id = $1"#)
                            .bind(node.id)
                            .fetch_optional(pool)
                            .await
                            .map_err(|_| Denial::ServerError)?;
                    Some(context::ParentView {
                        identifier: context::issue_identifier(
                            &node.project_identifier,
                            issue_parent_sequence(pool, &node.id).await?,
                        ),
                        title: name,
                        state_name,
                        work_branch,
                        description_stripped: description.and_then(|row| row.0),
                        comments_count,
                    })
                }
            }
        }
    };
    // Multi-level lineage only when a grandparent exists
    // (`len(ancestors) > 2`); each node keeps its own identifier.
    let lineage = if ancestors.len() > 2 {
        let mut nodes = Vec::new();
        for node in ancestors {
            nodes.push(context::LineageNode {
                identifier: context::issue_identifier(
                    &node.project_identifier,
                    issue_parent_sequence(pool, &node.id).await?,
                ),
                title: node.title.clone(),
            });
        }
        nodes
    } else {
        Vec::new()
    };
    Ok((parent, lineage))
}

/// Sequence lookup for a chain node (the walk carries identifiers, not
/// sequence ids).
async fn issue_parent_sequence(pool: &sqlx::PgPool, issue_id: &uuid::Uuid) -> Result<i64, Denial> {
    let row: Option<(i32,)> = sqlx::query_as(r#"SELECT sequence_id FROM issues WHERE id = $1"#)
        .bind(issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|r| i64::from(r.0)).ok_or(Denial::ServerError)
}

/// `_tick_context` (`context.py:341-398`): the issue's budget pool + clock.
/// `None` (JSON null) when no ticker row exists. Effective values resolve
/// through the project row: cap = pool + granted + waited (`-1` is
/// infinite), interval through the current phase's column.
async fn load_tick(
    pool: &sqlx::PgPool,
    project: &ProjectRow,
    state: &Option<StateRow>,
    issue_id: &uuid::Uuid,
) -> Result<Option<Value>, Denial> {
    let row: Option<(i32, i32, i32, bool)> = sqlx::query_as(
        r#"SELECT used, waited, granted, enabled FROM issue_agent_ticker WHERE issue_id = $1"#,
    )
    .bind(issue_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((used, waited, granted, enabled)) = row else {
        return Ok(None);
    };
    let cap = if project.pool == -1 {
        context::TickCap::Infinite
    } else {
        context::TickCap::Finite(project.pool + i64::from(granted) + i64::from(waited))
    };
    let wait_allowance = if project.pool == -1 {
        0
    } else {
        (project.pool - i64::from(waited)).max(0)
    };
    // `cadence_fields_for`: the registered ticking state's column, else
    // the implementation default (`agent_phases.py:191-200`).
    let interval_seconds = match state {
        Some(s) if s.group == "started" && s.name == "In Progress" => project.interval_impl,
        Some(s) if s.group == "review" && s.name == "In Review" => project.interval_review,
        Some(s) if s.group == "test" && s.name == "In Test" => project.interval_test,
        _ => project.interval_impl,
    };
    Ok(context::tick_context(&context::TickerView {
        used: i64::from(used),
        waited: i64::from(waited),
        enabled,
        cap,
        wait_allowance,
        interval_seconds,
    }))
}

/// `_parent_done_payload` (`context.py:401-416`): the stub run has no
/// parent, so the ticker-stashed resume parent decides; empty renders the
/// constant. Payloads render with sorted keys + 2-space indent
/// (`json.dumps(payload, indent=2, sort_keys=True)`).
async fn load_parent_payload(pool: &sqlx::PgPool, issue_id: &uuid::Uuid) -> Result<String, Denial> {
    let ticker: Option<(Option<uuid::Uuid>,)> = sqlx::query_as(
        r#"SELECT resume_parent_run_id FROM issue_agent_ticker WHERE issue_id = $1"#,
    )
    .bind(issue_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((Some(parent_id),)) = ticker else {
        return Ok(context::NO_PARENT_PAYLOAD_TEXT.to_owned());
    };
    let run: Option<(Option<Value>,)> =
        sqlx::query_as(r#"SELECT done_payload FROM agent_run WHERE id = $1"#)
            .bind(parent_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    match run.and_then(|r| r.0) {
        None | Some(Value::Null) => Ok(context::NO_PARENT_PAYLOAD_TEXT.to_owned()),
        Some(payload) => {
            if payload.is_null()
                || payload.as_object().is_some_and(serde_json::Map::is_empty)
                || payload.as_array().is_some_and(Vec::is_empty)
            {
                return Ok(context::NO_PARENT_PAYLOAD_TEXT.to_owned());
            }
            Ok(sorted_pretty(&payload))
        }
    }
}

/// `json.dumps(payload, indent=2, sort_keys=True)`: sorted keys, 2-space
/// indent, compact separators (`", "` item separator is `,\n`-joined at
/// indent 2 — matching CPython's `indent=2` rendering).
fn sorted_pretty(value: &Value) -> String {
    fn sort(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut sorted = serde_json::Map::with_capacity(map.len());
                for key in keys {
                    sorted.insert(key.clone(), sort(&map[key]));
                }
                Value::Object(sorted)
            }
            Value::Array(items) => Value::Array(items.iter().map(sort).collect()),
            _ => value.clone(),
        }
    }
    // `to_string_pretty` renders 2-space indents with `": "` separators,
    // matching CPython `json.dumps(indent=2)` for JSON scalars. Python
    // renders `True/False/None`; payloads crossing this path are JSON
    // column values, which carry no such scalars beyond what serde
    // already normalizes.
    serde_json::to_string_pretty(&sort(value)).unwrap_or_else(|_| "{}".to_owned())
}

/// `_scheduler_context` (`views.py:463-483`): the binding must live in
/// this workspace (tenant isolation); anything unresolvable is the 404.
/// The default manager drops soft-deleted bindings; the scheduler /
/// project follows are unfiltered FK reads.
async fn load_scheduler_context(
    pool: &sqlx::PgPool,
    workspace: &Workspace,
    data: &Value,
) -> Result<Value, Denial> {
    let binding_id = required_id(
        data,
        "binding_id",
        "binding_id is required for the scheduler kind",
    )?;
    let row: Option<BindingCols> = sqlx::query_as(
        r#"SELECT b.id, b.project_id, b.scheduler_id, b.extra_context, b.outcome_mode
           FROM scheduler_bindings b
           WHERE b.id = $1 AND b.workspace_id = $2 AND b.deleted_at IS NULL"#,
    )
    .bind(binding_id)
    .bind(workspace.id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((_, project_id, scheduler_id, extra_context, outcome_mode)) = row else {
        return Err(Denial::NotError("scheduler binding not found".to_owned()));
    };
    let scheduler: Option<SchedulerCols> =
        sqlx::query_as(r#"SELECT slug, name, description, prompt FROM schedulers WHERE id = $1"#)
            .bind(scheduler_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some((scheduler_slug, scheduler_name, scheduler_description, scheduler_prompt)) = scheduler
    else {
        return Err(Denial::ServerError);
    };
    let (project_id_str, project_identifier, project_name, project_description) = match project_id {
        Some(pid) => {
            let project = load_project(pool, &pid).await?;
            (
                project.id,
                project.identifier,
                project.name,
                project.description,
            )
        }
        None => (String::new(), String::new(), String::new(), None),
    };
    let outcome_directive =
        context::outcome_mode_directive(outcome_mode.as_deref().unwrap_or("")).to_owned();
    let input = context::SchedulerContextInput {
        workspace_slug: workspace.slug.clone(),
        workspace_name: workspace.name.clone(),
        project_id: if project_id_str.is_empty() {
            None
        } else {
            Some(project_id_str)
        },
        project_identifier,
        project_name,
        project_description,
        scheduler_slug,
        scheduler_name,
        scheduler_description,
        // `_FakeRun`: fresh id, `local_runner` executor, empty tool plan.
        run_id: uuid::Uuid::new_v4().to_string(),
        executor_kind: "local_runner".to_owned(),
        available_tools: Value::Null,
        unavailable_capabilities: Value::Null,
        extra_toolsets_enabled: false,
        extra_toolsets_schema_tool: String::new(),
        limits: Value::Null,
        scheduler_prompt: scheduler_prompt.unwrap_or_default(),
        binding_extra_context: extra_context.unwrap_or_default(),
        outcome_directive,
    };
    Ok(context::build_scheduler_context(&input))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query_map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn scope_defaults_to_user() {
        assert_eq!(resolve_scope(&query_map(&[])), SCOPE_USER);
        assert_eq!(resolve_scope(&query_map(&[("scope", "bogus")])), SCOPE_USER);
        assert_eq!(
            resolve_scope(&query_map(&[("scope", "workspace")])),
            SCOPE_WORKSPACE
        );
    }

    #[test]
    fn put_scope_prefers_truthy_body() {
        let query = query_map(&[("scope", "workspace")]);
        // Truthy body scope wins over the query param.
        assert_eq!(
            put_scope(&serde_json::json!({"scope": "user"}), &query).expect("scope"),
            SCOPE_USER
        );
        assert_eq!(
            put_scope(&serde_json::json!({"scope": "workspace"}), &query).expect("scope"),
            SCOPE_WORKSPACE
        );
        // Falsy body scopes fall back to the query param.
        assert_eq!(
            put_scope(&serde_json::json!({"scope": ""}), &query).expect("scope"),
            SCOPE_WORKSPACE
        );
        assert_eq!(
            put_scope(&serde_json::json!({}), &query_map(&[])).expect("scope"),
            SCOPE_USER
        );
        // A truthy non-string body scope never equals "workspace".
        assert_eq!(
            put_scope(&serde_json::json!({"scope": 5}), &query).expect("scope"),
            SCOPE_USER
        );
    }

    #[test]
    fn error_detail_keeps_wire_order() {
        assert_eq!(
            ordered_error_detail("render failed", "boom"),
            r#"{"error":"render failed","detail":"boom"}"#
        );
    }

    #[test]
    fn unknown_kind_names_kinds_in_order() {
        let (status, body) = Denial::BadKind("nope".to_owned()).status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            r#"{"error":"unknown kind 'nope'","kinds":["coding-task","review","test","scheduler"]}"#
        );
    }

    #[test]
    fn template_name_follows_ticking_states() {
        let started = StateRow {
            name: "In Progress".to_owned(),
            group: "started".to_owned(),
        };
        assert_eq!(template_name_for(Some(&started)), "coding-task");
        let review = StateRow {
            name: "In Review".to_owned(),
            group: "review".to_owned(),
        };
        assert_eq!(template_name_for(Some(&review)), "review");
        let custom = StateRow {
            name: "Custom".to_owned(),
            group: "started".to_owned(),
        };
        assert_eq!(template_name_for(Some(&custom)), "coding-task");
        assert_eq!(template_name_for(None), "coding-task");
    }

    #[test]
    fn comment_timestamps_use_isoformat_offset() {
        let dt = chrono::DateTime::parse_from_rfc3339("2026-09-28T04:52:20.217834Z")
            .expect("parse")
            .with_timezone(&chrono::Utc);
        assert_eq!(render_isoformat(dt), "2026-09-28T04:52:20.217834+00:00");
        let whole = chrono::DateTime::parse_from_rfc3339("2026-09-28T04:52:20Z")
            .expect("parse")
            .with_timezone(&chrono::Utc);
        assert_eq!(render_isoformat(whole), "2026-09-28T04:52:20+00:00");
        // Millisecond-exact micros keep six digits (where chrono AutoSi
        // would trim to ".123"): Python `isoformat()` always prints six.
        let millis = chrono::DateTime::parse_from_rfc3339("2026-09-28T04:52:20.123Z")
            .expect("parse")
            .with_timezone(&chrono::Utc);
        assert_eq!(render_isoformat(millis), "2026-09-28T04:52:20.123000+00:00");
    }

    #[test]
    fn parent_payload_sorts_keys_like_python() {
        let payload = serde_json::json!({"z": 1, "a": {"d": 4, "c": 3}});
        assert_eq!(
            sorted_pretty(&payload),
            "{\n  \"a\": {\n    \"c\": 3,\n    \"d\": 4\n  },\n  \"z\": 1\n}"
        );
    }

    #[test]
    fn csrf_unmask_matches_django_vector() {
        // Vector from Django 4.2 `csrf._mask_cipher_secret`:
        // secret "abcdefghijklmnopqrstuvwxyz012345" masked as below.
        let masked = "YST0mOcUxCDEc9kIkl6F1wOAEYGoc1ibYTV3qTi1FLNPomyXACoYlRaX2nwf4Uc6";
        let secret = "abcdefghijklmnopqrstuvwxyz012345";
        assert_eq!(unmask_csrf_token(masked), secret);
        assert!(csrf_token_matches(masked, secret));
        assert!(csrf_token_matches(secret, secret));
        assert!(!csrf_token_matches(
            masked,
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
        ));
        assert!(check_csrf_format(masked).is_ok());
        assert!(check_csrf_format(secret).is_ok());
        assert_eq!(
            check_csrf_format("short").unwrap_err(),
            "has incorrect length"
        );
        assert_eq!(
            check_csrf_format(&"!".repeat(32)).unwrap_err(),
            "has invalid characters"
        );
    }

    #[test]
    fn csrf_origin_verification_matches_django() {
        let trusted: Vec<String> = vec![];
        assert!(origin_verified("http://app.test", "app.test", &trusted));
        assert!(!origin_verified("http://evil.test", "app.test", &trusted));
        let trusted = vec!["https://app.example.com".to_owned()];
        assert!(origin_verified(
            "https://app.example.com",
            "other.test",
            &trusted
        ));
        let trusted = vec!["https://*.example.com".to_owned()];
        assert!(origin_verified(
            "https://a.example.com",
            "other.test",
            &trusted
        ));
        assert!(origin_verified(
            "https://example.com",
            "other.test",
            &trusted
        ));
        assert!(!origin_verified("https://evil.com", "other.test", &trusted));
    }

    #[test]
    fn json_truthy_matches_python_or() {
        assert!(json_truthy(&serde_json::json!("x")));
        assert!(json_truthy(&serde_json::json!(5)));
        assert!(json_truthy(&serde_json::json!(["a"])));
        assert!(!json_truthy(&serde_json::json!("")));
        assert!(!json_truthy(&serde_json::json!(0)));
        assert!(!json_truthy(&serde_json::json!(false)));
        assert!(!json_truthy(&serde_json::json!(null)));
        assert!(!json_truthy(&serde_json::json!([])));
        assert!(!json_truthy(&serde_json::json!({})));
    }
}
