//! The GitHub leg of the cloud-agent tools (`utils/github_client.py:38-220`
//! + `utils/github_app_auth.py:63-193`), shared by both binaries.
//!
//! The ported [`GithubClient`][pidash_services::integrations::adapters_github::GithubClient]
//! trait covers only the endpoints the sync adapter touches; the twin tools
//! need `get_file`, the pull-request aspects, and `for_installation`, so the
//! REST subset lives here, mirroring `handlers_pr_links.rs`' transport
//! precedent (JWT claims, config reads, token mint) line for line where
//! they overlap. Every failure carries the Python exception's type name
//! plus its message ([`GithubFailure`]): the twin masks the message to
//! `Error calling tool 'name'`, but the read-path ledger records the
//! type as `error_code` (`tools.py:147`), so the names must match
//! CPython (`GithubAuthError`, `KeyError`, `ValueError`, ...).
//!
//! Two deliberate approximations, both unreachable against real GitHub:
//! non-string `token` / `expires_at` payload fields (Python would
//! `f-string`-format or `AttributeError` them) and non-array paginated
//! bodies (Python would iterate or `TypeError` them).

#![forbid(unsafe_code)]

use std::collections::HashMap;

use base64::Engine as _;

use pidash_db::app_integrations::queries_github::{
    app_headers, build_app_jwt_claims, installation_token_cache_key, installation_token_ttl_secs,
    next_url, normalize_private_key, parse_github_datetime, DEFAULT_TIMEOUT_SECONDS,
    GITHUB_ACCEPT_HEADER, GITHUB_API_BASE, GITHUB_API_VERSION_HEADER,
};
use pidash_db::redis::RedisHandle;
use serde_json::Value;
use sqlx::PgPool;

/// `GithubClient._headers` (`github_client.py:54-60`): the user-agent is
/// `pi-dash-github-sync` here, not the app's `pi-dash-github-app`.
pub const CLIENT_USER_AGENT: &str = "pi-dash-github-sync";

/// `get_file` decoded-size cap (`github_client.py:176`).
pub const FILE_RESULT_LIMIT_BYTES: usize = 65536;

/// `get_pull_request_diff` truncation cap (`github_client.py:197`).
pub const DIFF_RESULT_LIMIT_BYTES: usize = 65536;

/// `list_pull_request_files` cap (`github_client.py:202`).
pub const PR_FILES_LIMIT: usize = 300;

/// `list_pull_request_reviews` / comments caps (`github_client.py:210,216`).
pub const PR_LIST_LIMIT: usize = 200;

/// One GitHub-leg failure: the Python exception's `__name__` plus its
/// `str`. The twin masks the message; the ledger records the type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubFailure {
    /// `type(exc).__name__` (`GithubAuthError`, `KeyError`, ...).
    pub python_type: &'static str,
    /// `str(exc)`.
    pub message: String,
}

impl GithubFailure {
    fn new(python_type: &'static str, message: impl Into<String>) -> Self {
        Self {
            python_type,
            message: message.into(),
        }
    }

    /// `GithubAuthError` (`github_client.py:26-27`): 401s and empty tokens.
    pub fn auth(message: impl Into<String>) -> Self {
        Self::new("GithubAuthError", message)
    }

    /// `GithubPermissionError` (`github_client.py:30-31`): 403s.
    pub fn permission(message: impl Into<String>) -> Self {
        Self::new("GithubPermissionError", message)
    }

    /// `GithubNotFoundError` (`github_client.py:34-35`): 404s.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("GithubNotFoundError", message)
    }

    /// `requests.HTTPError` (`models.py:1017-1025`):
    /// `raise_for_status` on any other non-2xx — no body.
    pub fn http_error(status: reqwest::StatusCode, _body: &str, url: &str) -> Self {
        let reason = status.canonical_reason().unwrap_or("Error");
        let kind = if status.is_server_error() {
            "Server Error"
        } else {
            "Client Error"
        };
        Self::new(
            "HTTPError",
            format!("{} {kind}: {reason} for url: {url}", status.as_u16()),
        )
    }

    /// `GithubAppConfigError` (`github_app_auth.py:26-27`).
    pub fn config(message: impl Into<String>) -> Self {
        Self::new("GithubAppConfigError", message)
    }

    /// `GithubAppAuthError` (`github_app_auth.py:30-31`).
    pub fn app_auth(message: impl Into<String>) -> Self {
        Self::new("GithubAppAuthError", message)
    }

    /// `ValueError`: arg guards, bad ints, bad PEMs (probed: PyJWT
    /// surfaces a bare `ValueError` for an unloadable key).
    pub fn value_error(message: impl Into<String>) -> Self {
        Self::new("ValueError", message)
    }

    /// `KeyError`: `pull["head"]["sha"]` misses. `str` renders the
    /// missing key `repr`-quoted, as CPython does.
    pub fn key_error(key: &str) -> Self {
        Self::new("KeyError", format!("'{key}'"))
    }

    /// `TypeError`: non-iterable paginated bodies, non-object heads.
    pub fn type_error(message: impl Into<String>) -> Self {
        Self::new("TypeError", message)
    }

    /// `binascii.Error` (`__name__` is `Error`): undecodable contents.
    pub fn binascii(message: impl Into<String>) -> Self {
        Self::new("Error", message)
    }

    /// `json.JSONDecodeError` (a `ValueError`): non-JSON bodies.
    pub fn json_decode(message: impl Into<String>) -> Self {
        Self::new("JSONDecodeError", message)
    }

    /// `requests.exceptions.InvalidHeader`: header values carrying
    /// control bytes (a token with `\r`/`\n`), which `requests`
    /// rejects at send time — the `_headers` failure mode.
    pub fn invalid_header(message: impl Into<String>) -> Self {
        Self::new("InvalidHeader", message)
    }

    /// `requests` transport failures, by kind: timeouts split into
    /// connect vs read (`ConnectTimeout` / `ReadTimeout`), connects
    /// into `ConnectionError`, anything else into `RequestException`.
    pub fn transport(error: &reqwest::Error) -> Self {
        if error.is_timeout() {
            if error.is_connect() {
                return Self::new("ConnectTimeout", error.to_string());
            }
            return Self::new("ReadTimeout", error.to_string());
        }
        if error.is_connect() {
            return Self::new("ConnectionError", error.to_string());
        }
        if error.is_builder() {
            return Self::new("InvalidURL", error.to_string());
        }
        Self::new("RequestException", error.to_string())
    }

    /// Redis cache failures (`django.core.cache` over django-redis
    /// surfaces `redis.exceptions.ConnectionError`).
    pub fn cache(message: impl Into<String>) -> Self {
        Self::new("ConnectionError", message)
    }
}

impl std::fmt::Display for GithubFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for GithubFailure {}

// ---------------------------------------------------------------------------
// App auth (`github_app_auth.py:63-193`)
// ---------------------------------------------------------------------------

/// What the token mint needs: pool (app-config reads), cache
/// (installation-token cache), instance secret (config keyring), and
/// the API base (tests point it at a mock server; production is
/// [`GITHUB_API_BASE`]).
#[derive(Clone)]
pub struct GithubAppContext {
    pub pool: PgPool,
    pub redis: Option<RedisHandle>,
    pub secret: String,
    pub api_base: String,
}

impl GithubAppContext {
    pub fn new(pool: PgPool, redis: Option<RedisHandle>, secret: String) -> Self {
        Self {
            pool,
            redis,
            secret,
            api_base: GITHUB_API_BASE.to_owned(),
        }
    }
}

/// `require_github_app_config()` (`github_app_auth.py:63-74`): app id +
/// slug + private key, each stripped, missing → the config error.
/// Mirrors `handlers_pr_links.rs::github_app_config` (registry reads +
/// value coercion + `normalize_private_key`); returns only what the
/// mint consumes (id + key).
pub async fn require_github_app_config(
    ctx: &GithubAppContext,
) -> Result<(String, String), GithubFailure> {
    let store = pidash_db::config::PgConfigStore::new(ctx.pool.clone());
    let registry = pidash_db::config::registry::global();
    let keyring = pidash_services::license::encryption::Keyring::from_secret(&ctx.secret);
    let values: HashMap<String, pidash_db::config::ConfigValue> =
        pidash_db::config::accessor::get_many_in(
            registry,
            &store,
            &keyring,
            &["GITHUB_APP_ID", "GITHUB_APP_SLUG", "GITHUB_APP_PRIVATE_KEY"],
        )
        .await
        .unwrap_or_default();
    let string = |key: &str| match values.get(key) {
        Some(pidash_db::config::ConfigValue::Str(value)) => value.clone(),
        Some(pidash_db::config::ConfigValue::Int(value)) => value.to_string(),
        Some(pidash_db::config::ConfigValue::Float(value)) => value.to_string(),
        Some(pidash_db::config::ConfigValue::Bool(true)) => "True".to_owned(),
        Some(pidash_db::config::ConfigValue::Bool(false)) => "False".to_owned(),
        _ => String::new(),
    };
    let app_id = string("GITHUB_APP_ID");
    let app_slug = string("GITHUB_APP_SLUG");
    let private_key = string("GITHUB_APP_PRIVATE_KEY");
    let config = (
        app_id.trim().to_owned(),
        app_slug.trim().to_owned(),
        normalize_private_key(Some(&private_key)),
    );
    let mut missing: Vec<&str> = Vec::new();
    if config.0.is_empty() {
        missing.push("app_id");
    }
    if config.1.is_empty() {
        missing.push("app_slug");
    }
    if config.2.is_empty() {
        missing.push("private_key");
    }
    if !missing.is_empty() {
        return Err(GithubFailure::config(format!(
            "GitHub App config missing: {}",
            missing.join(", ")
        )));
    }
    Ok((config.0, config.2))
}

/// `build_app_jwt` (`github_app_auth.py:77-89`): RS256 over
/// `iat=now-60, exp=now+540, iss=app_id`, `typ: JWT`. Mirrors
/// `handlers_pr_links.rs::build_app_jwt_token`; an unloadable key is
/// the probed bare `ValueError`.
pub fn build_app_jwt(app_id: &str, private_key: &str) -> Result<String, GithubFailure> {
    let now = chrono::Utc::now().timestamp();
    let claims = build_app_jwt_claims(now, app_id);
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(private_key.as_bytes())
        .map_err(|error| GithubFailure::value_error(error.to_string()))?;
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.typ = Some("JWT".to_owned());
    #[derive(serde::Serialize)]
    struct Claims {
        iat: i64,
        exp: i64,
        iss: String,
    }
    jsonwebtoken::encode(
        &header,
        &Claims {
            iat: claims.iat,
            exp: claims.exp,
            iss: claims.iss,
        },
        &key,
    )
    .map_err(|error| GithubFailure::value_error(error.to_string()))
}

/// `mint_installation_token` (`github_app_auth.py:164-171`): POST the
/// access-tokens endpoint under the app JWT with the 30s default
/// timeout; `raise_for_status`, decoded JSON.
async fn mint_installation_token(
    http: &reqwest::Client,
    api_base: &str,
    jwt: &str,
    installation_id: i64,
) -> Result<Value, GithubFailure> {
    let url = format!("{api_base}/app/installations/{installation_id}/access_tokens");
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in app_headers(jwt) {
        headers.insert(
            reqwest::header::HeaderName::from_bytes(name.as_bytes()).expect("app header name"),
            value.parse().expect("app header value"),
        );
    }
    let response = http
        .post(&url)
        .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECONDS))
        .headers(headers)
        .send()
        .await
        .map_err(|error| GithubFailure::transport(&error))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(GithubFailure::http_error(status, &text, &url));
    }
    serde_json::from_str(&text).map_err(|error| GithubFailure::json_decode(error.to_string()))
}

/// `installation_token` (`github_app_auth.py:174-193`): cached token or
/// a fresh mint. Without Redis the mint runs through (the cache only
/// saves the POST). Cache errors fail the call, as Django's cache
/// would raise; a missing token is `GithubAppAuthError`; the TTL is
/// `max(60, expires-now-60)` defaulting to 55 minutes.
pub async fn installation_token(
    ctx: &GithubAppContext,
    http: &reqwest::Client,
    installation_id: i64,
) -> Result<String, GithubFailure> {
    let cache_key = installation_token_cache_key(installation_id);
    if let Some(redis) = ctx.redis.as_ref() {
        match redis.get_string(&cache_key).await {
            Ok(Some(token)) if !token.is_empty() => return Ok(token),
            Ok(_) => {}
            Err(error) => return Err(GithubFailure::cache(error.to_string())),
        }
    }
    let (app_id, private_key) = require_github_app_config(ctx).await?;
    let jwt = build_app_jwt(&app_id, &private_key)?;
    let payload = mint_installation_token(
        http,
        ctx.api_base.trim_end_matches('/'),
        &jwt,
        installation_id,
    )
    .await?;
    let token = payload
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if token.is_empty() {
        return Err(GithubFailure::app_auth(
            "GitHub did not return an installation token",
        ));
    }
    if let Some(redis) = ctx.redis.as_ref() {
        let remaining = parse_github_datetime(payload.get("expires_at").and_then(Value::as_str))
            .map_err(|error| GithubFailure::value_error(error.to_string()))?
            .map(|expires| (expires - chrono::Utc::now()).num_seconds());
        let ttl = installation_token_ttl_secs(remaining);
        if let Err(error) = redis
            .set_ex(&cache_key, &token, u64::try_from(ttl).unwrap_or(60))
            .await
        {
            return Err(GithubFailure::cache(error.to_string()));
        }
    }
    Ok(token)
}

// ---------------------------------------------------------------------------
// REST client (`github_client.py:38-220`, the twin subset)
// ---------------------------------------------------------------------------

/// Header-construction failure from [`github_headers`]: the fixed
/// per-header message (e.g. `"github bearer token"`). Callers map it onto
/// their own transport failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubHeadersError {
    /// Fixed message naming the header that failed to parse.
    pub message: String,
}

impl GithubHeadersError {
    fn new(message: &str) -> Self {
        Self {
            message: message.to_owned(),
        }
    }
}

impl std::fmt::Display for GithubHeadersError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for GithubHeadersError {}

/// `GithubClient._headers` (`github_client.py:54-60`) as a `HeaderMap`:
/// Bearer token + Accept + X-GitHub-Api-Version + User-Agent.
///
/// The canonical copy (PIDASHCONV-1625): fallible, because the token
/// is attacker-influenced and header construction must never panic.
/// The messages keep the overlay's texts so OVL-10's switch is
/// message-preserving.
pub fn github_headers(token: &str) -> Result<reqwest::header::HeaderMap, GithubHeadersError> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {token}")
            .parse()
            .map_err(|_| GithubHeadersError::new("github bearer token"))?,
    );
    headers.insert(
        reqwest::header::ACCEPT,
        GITHUB_ACCEPT_HEADER
            .parse()
            .map_err(|_| GithubHeadersError::new("github accept header"))?,
    );
    headers.insert(
        "X-GitHub-Api-Version",
        GITHUB_API_VERSION_HEADER
            .parse()
            .map_err(|_| GithubHeadersError::new("github version header"))?,
    );
    headers.insert(
        reqwest::header::USER_AGENT,
        CLIENT_USER_AGENT
            .parse()
            .map_err(|_| GithubHeadersError::new("github user agent"))?,
    );
    Ok(headers)
}

/// `GithubClient` (`github_client.py:38-52`): bearer-token REST over
/// `api_base` with a per-request timeout. Only the endpoints the twin
/// tools call are ported; each returns decoded JSON.
#[derive(Debug)]
pub struct GithubRestClient {
    http: reqwest::Client,
    token: String,
    timeout_secs: u64,
    api_base: String,
}

/// `raw = response.text.encode()[:65536]`
/// (`github_client.py:197-198`): decode (utf-8, `replace`),
/// re-encode, cut at 64 KiB, decode again — then
/// `{"diff", "truncated": len(content) > len(raw)}`. The cut lands
/// on re-encoded bytes, so replacement expansion shifts it for
/// non-UTF8 bodies. PY decodes via requests `apparent_encoding`,
/// probed byte-equal to utf-8/`replace` for every valid-UTF8 body —
/// only non-UTF8 bytes (pathological for diffs) can differ.
fn truncate_diff(content: &[u8]) -> Value {
    let text = String::from_utf8_lossy(content);
    let encoded = text.as_bytes();
    let take = encoded.len().min(DIFF_RESULT_LIMIT_BYTES);
    let raw = &encoded[..take];
    let mut out = serde_json::Map::with_capacity(2);
    out.insert(
        "diff".to_owned(),
        Value::String(String::from_utf8_lossy(raw).into_owned()),
    );
    out.insert(
        "truncated".to_owned(),
        Value::Bool(content.len() > raw.len()),
    );
    Value::Object(out)
}

/// What `head["sha"]` raises when `head` is present but not an
/// object (CPython subscript errors).
fn subscript_error(head: &Value) -> String {
    match head {
        Value::String(_) => "string indices must be integers".to_owned(),
        Value::Array(_) => "list indices must be integers or slices, not str".to_owned(),
        Value::Number(number) if number.is_f64() => {
            "'float' object is not subscriptable".to_owned()
        }
        Value::Number(_) => "'int' object is not subscriptable".to_owned(),
        Value::Bool(_) => "'bool' object is not subscriptable".to_owned(),
        Value::Null | Value::Object(_) => unreachable!("handled by the caller"),
    }
}

impl GithubRestClient {
    /// `GithubClient(token, timeout, api_base)` (`:39-44`): empty
    /// tokens raise before anything else; the base loses its trailing
    /// slash.
    pub fn new(token: &str, timeout_secs: u64, api_base: &str) -> Result<Self, GithubFailure> {
        if token.is_empty() {
            return Err(GithubFailure::auth("empty token"));
        }
        Ok(Self {
            http: reqwest::Client::new(),
            token: token.to_owned(),
            timeout_secs,
            api_base: api_base.trim_end_matches('/').to_owned(),
        })
    }

    /// `GithubClient.for_installation` (`:47-52`): mint (or reuse) the
    /// installation token, then [`GithubRestClient::new`].
    pub async fn for_installation(
        ctx: &GithubAppContext,
        http: &reqwest::Client,
        installation_id: i64,
        timeout_secs: u64,
    ) -> Result<Self, GithubFailure> {
        let token = installation_token(ctx, http, installation_id).await?;
        Self::new(&token, timeout_secs, &ctx.api_base)
    }

    /// `GithubClient._headers` (`:54-60`), via the [`github_headers`]
    /// canonical: a bad token is `InvalidHeader`, never a panic
    /// (PIDASHCONV-1625).
    fn headers(&self) -> Result<reqwest::header::HeaderMap, GithubFailure> {
        github_headers(&self.token).map_err(|error| GithubFailure::invalid_header(error.message))
    }

    /// `GithubClient._request` (`:62-74`): timeout + default headers,
    /// then 401/403/404 → typed errors carrying the response text,
    /// anything else non-2xx → `raise_for_status`.
    async fn request(
        &self,
        method: reqwest::Method,
        url: &str,
        accept: Option<&str>,
    ) -> Result<reqwest::Response, GithubFailure> {
        let mut headers = self.headers()?;
        if let Some(accept) = accept {
            headers.insert(
                reqwest::header::ACCEPT,
                accept.parse().expect("accept override"),
            );
        }
        let response = self
            .http
            .request(method, url)
            .timeout(std::time::Duration::from_secs(self.timeout_secs.max(1)))
            .headers(headers)
            .send()
            .await
            .map_err(|error| GithubFailure::transport(&error))?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            let text = response.text().await.unwrap_or_default();
            return Err(GithubFailure::auth(text));
        }
        if status == reqwest::StatusCode::FORBIDDEN {
            let text = response.text().await.unwrap_or_default();
            return Err(GithubFailure::permission(text));
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            let text = response.text().await.unwrap_or_default();
            return Err(GithubFailure::not_found(text));
        }
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(GithubFailure::http_error(status, &text, url));
        }
        Ok(response)
    }

    /// Decode one JSON response body (`response.json()`).
    async fn json(response: reqwest::Response) -> Result<Value, GithubFailure> {
        let text = response
            .text()
            .await
            .map_err(|error| GithubFailure::transport(&error))?;
        serde_json::from_str(&text).map_err(|error| GithubFailure::json_decode(error.to_string()))
    }

    /// `GithubClient._next_url` (`:76-85`), via the
    /// [`next_url`][pidash_db::app_integrations::queries_github::next_url]
    /// canonical (PIDASHCONV-1625).
    fn next_url(link: &str) -> Option<String> {
        next_url(link)
    }

    /// `GithubClient._paginate` (`:87-97`): follow `next` links,
    /// yielding every item. `query` is the pre-rendered
    /// `urlencode(params)` tail (each call site renders its own fixed
    /// params, dict order).
    async fn paginate(&self, path: &str, query: &str) -> Result<Vec<Value>, GithubFailure> {
        let mut url = format!("{}{path}", self.api_base);
        if !query.is_empty() {
            url = format!("{url}?{query}");
        }
        let mut items = Vec::new();
        while !url.is_empty() {
            let response = self.request(reqwest::Method::GET, &url, None).await?;
            let link = response
                .headers()
                .get(reqwest::header::LINK)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_owned();
            let body = Self::json(response).await?;
            let Value::Array(page) = body else {
                return Err(GithubFailure::type_error(format!(
                    "expected a JSON array from GitHub, got {}",
                    json_kind(&body)
                )));
            };
            items.extend(page);
            url = Self::next_url(&link).unwrap_or_default();
        }
        Ok(items)
    }

    /// `get_pull_request` (`:162-165`).
    pub async fn get_pull_request(
        &self,
        owner: &str,
        name: &str,
        number: i64,
    ) -> Result<Value, GithubFailure> {
        let url = format!("{}/repos/{owner}/{name}/pulls/{number}", self.api_base);
        let response = self.request(reqwest::Method::GET, &url, None).await?;
        Self::json(response).await
    }

    /// `get_file` (`:167-189`): contents API, `type == "file"` guard,
    /// forgiving base64 (non-alphabet chars discarded, as
    /// `validate=False` does), the 64 KiB cap, UTF-8-or-binary shape.
    /// `path` interpolates raw except ASCII controls (encoded as
    /// requests sends them, so the `url` crate cannot strip them and
    /// resolve the resulting dot segments out of the bound repo) and
    /// `ref` is `quote_plus`-encoded, exactly as Python builds the URL.
    pub async fn get_file(
        &self,
        owner: &str,
        name: &str,
        path: &str,
        git_ref: &str,
    ) -> Result<Value, GithubFailure> {
        let path = encode_path_controls(path);
        let mut url = format!("{}/repos/{owner}/{name}/contents/{path}", self.api_base);
        if !git_ref.is_empty() {
            url = format!("{url}?ref={}", quote_plus(git_ref));
        }
        let response = self.request(reqwest::Method::GET, &url, None).await?;
        let payload = Self::json(response).await?;
        if payload.get("type").and_then(Value::as_str) != Some("file") {
            return Err(GithubFailure::value_error("path is not a file"));
        }
        let encoded = payload.get("content").and_then(Value::as_str).unwrap_or("");
        let cleaned: String = encoded
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '='))
            .collect();
        let raw = base64_engine()
            .decode(cleaned.as_bytes())
            .map_err(|error| GithubFailure::binascii(error.to_string()))?;
        if raw.len() > FILE_RESULT_LIMIT_BYTES {
            return Err(GithubFailure::value_error(
                "file exceeds Cloud Agent result limit",
            ));
        }
        let mut out = serde_json::Map::with_capacity(4);
        out.insert(
            "path".to_owned(),
            payload.get("path").cloned().unwrap_or(Value::Null),
        );
        out.insert(
            "sha".to_owned(),
            payload.get("sha").cloned().unwrap_or(Value::Null),
        );
        match String::from_utf8(raw) {
            Ok(content) => {
                out.insert("content".to_owned(), Value::String(content));
            }
            Err(_) => {
                out.insert("content".to_owned(), Value::String(String::new()));
                out.insert(
                    "unsupported_content".to_owned(),
                    Value::String("binary".to_owned()),
                );
            }
        }
        Ok(Value::Object(out))
    }

    /// `get_pull_request_diff` (`:191-198`): the diff media type,
    /// truncated to the first 64 KiB through the
    /// `response.text.encode()[:65536]` round-trip.
    pub async fn get_pull_request_diff(
        &self,
        owner: &str,
        name: &str,
        number: i64,
    ) -> Result<Value, GithubFailure> {
        let url = format!("{}/repos/{owner}/{name}/pulls/{number}", self.api_base);
        let response = self
            .request(
                reqwest::Method::GET,
                &url,
                Some("application/vnd.github.diff"),
            )
            .await?;
        let bytes = response
            .bytes()
            .await
            .map_err(|error| GithubFailure::transport(&error))?;
        Ok(truncate_diff(&bytes))
    }

    /// `list_pull_request_files` (`:200-202`): all pages, first 300.
    pub async fn list_pull_request_files(
        &self,
        owner: &str,
        name: &str,
        number: i64,
    ) -> Result<Vec<Value>, GithubFailure> {
        let mut files = self
            .paginate(
                &format!("/repos/{owner}/{name}/pulls/{number}/files"),
                "per_page=100",
            )
            .await?;
        files.truncate(PR_FILES_LIMIT);
        Ok(files)
    }

    /// `list_pull_request_checks` (`:203-206`): the PR's head sha,
    /// then its check-runs. `pull["head"]["sha"]` subscripts, so a
    /// missing key is `KeyError`, a non-object `head` is `TypeError`,
    /// and a non-string `sha` formats into the URL.
    pub async fn list_pull_request_checks(
        &self,
        owner: &str,
        name: &str,
        number: i64,
    ) -> Result<Value, GithubFailure> {
        let pull = self.get_pull_request(owner, name, number).await?;
        let head = pull
            .get("head")
            .ok_or_else(|| GithubFailure::key_error("head"))?;
        let sha_value = match head {
            Value::Object(map) => map
                .get("sha")
                .ok_or_else(|| GithubFailure::key_error("sha"))?,
            Value::Null => {
                return Err(GithubFailure::type_error(
                    "'NoneType' object is not subscriptable",
                ));
            }
            other => return Err(GithubFailure::type_error(subscript_error(other))),
        };
        // An f-string formats anything (`str`, i.e. `repr` except for
        // strings themselves).
        let sha = match sha_value {
            Value::String(sha) => sha.clone(),
            other => super::provider::py_repr(other),
        };
        let url = format!(
            "{}/repos/{owner}/{name}/commits/{sha}/check-runs",
            self.api_base
        );
        let response = self.request(reqwest::Method::GET, &url, None).await?;
        Self::json(response).await
    }

    /// `list_pull_request_reviews` (`:208-209`): all pages, first 200.
    pub async fn list_pull_request_reviews(
        &self,
        owner: &str,
        name: &str,
        number: i64,
    ) -> Result<Vec<Value>, GithubFailure> {
        let mut reviews = self
            .paginate(
                &format!("/repos/{owner}/{name}/pulls/{number}/reviews"),
                "per_page=100",
            )
            .await?;
        reviews.truncate(PR_LIST_LIMIT);
        Ok(reviews)
    }

    /// `list_pull_request_comments` (`:211-220`): issue comments plus
    /// review comments, each first 200.
    pub async fn list_pull_request_comments(
        &self,
        owner: &str,
        name: &str,
        number: i64,
    ) -> Result<Value, GithubFailure> {
        let mut issue_comments = self
            .paginate(
                &format!("/repos/{owner}/{name}/issues/{number}/comments"),
                "per_page=100&sort=created&direction=asc",
            )
            .await?;
        issue_comments.truncate(PR_LIST_LIMIT);
        let mut review_comments = self
            .paginate(
                &format!("/repos/{owner}/{name}/pulls/{number}/comments"),
                "per_page=100",
            )
            .await?;
        review_comments.truncate(PR_LIST_LIMIT);
        let mut out = serde_json::Map::with_capacity(2);
        out.insert("issue_comments".to_owned(), Value::Array(issue_comments));
        out.insert("review_comments".to_owned(), Value::Array(review_comments));
        Ok(Value::Object(out))
    }
}

/// Percent-encode ASCII control bytes in a contents `path`
/// (`github_client.py:170`, PIDASHCONV-1486): requests/urllib3 sends
/// them `%XX` (uppercase) and never resolves dot segments, while the
/// `url` crate strips tab/LF/CR and then resolves the resulting
/// `..` segments out of the bound repository. Only bytes below
/// `0x20` and `0x7f` change; `/`, `.` and everything else
/// interpolate raw, exactly as before.
fn encode_path_controls(path: &str) -> String {
    if !path.bytes().any(|byte| byte < 0x20 || byte == 0x7f) {
        return path.to_owned();
    }
    let mut out = Vec::with_capacity(path.len());
    for byte in path.bytes() {
        if byte < 0x20 || byte == 0x7f {
            out.extend_from_slice(format!("%{byte:02X}").as_bytes());
        } else {
            out.push(byte);
        }
    }
    String::from_utf8(out).expect("control-encoded path stays UTF-8")
}

/// `urllib.parse.quote_plus`, for the contents `ref` param: letters,
/// digits and `_.-~` pass, space becomes `+`, anything else is
/// percent-encoded uppercase over UTF-8 bytes.
fn quote_plus(value: &str) -> String {
    const SAFE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_.-~";
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        if SAFE.contains(byte) {
            out.push(*byte as char);
        } else if *byte == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The standard base64 engine (the contents `content` alphabet).
fn base64_engine() -> base64::engine::GeneralPurpose {
    use base64::engine::general_purpose::STANDARD;
    STANDARD
}

/// The JSON kind name for the non-array paginated-body error.
fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::IntoResponse;
    use std::sync::{Arc, Mutex};

    /// One scripted reply: status, optional Link header, body.
    #[derive(Clone)]
    struct Reply {
        status: StatusCode,
        link: Option<String>,
        body: String,
    }

    #[derive(Clone, Default)]
    struct MockState {
        headers: Arc<Mutex<Vec<HeaderMap>>>,
        urls: Arc<Mutex<Vec<String>>>,
        replies: Arc<Mutex<Vec<Reply>>>,
    }

    async fn handler(
        axum::extract::State(state): axum::extract::State<MockState>,
        headers: HeaderMap,
        uri: axum::http::Uri,
    ) -> impl IntoResponse {
        state.headers.lock().unwrap().push(headers);
        state.urls.lock().unwrap().push(uri.to_string());
        let reply = state.replies.lock().unwrap().remove(0);
        let mut response = (reply.status, reply.body).into_response();
        if let Some(link) = reply.link {
            response
                .headers_mut()
                .insert(reqwest::header::LINK, link.parse().expect("link header"));
        }
        response
    }

    async fn mock_server(replies: Vec<Reply>) -> (String, MockState) {
        let state = MockState {
            headers: Arc::new(Mutex::new(Vec::new())),
            urls: Arc::new(Mutex::new(Vec::new())),
            replies: Arc::new(Mutex::new(replies)),
        };
        let app = axum::Router::new()
            .fallback(handler)
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (base, state)
    }

    fn json_reply(body: &str) -> Reply {
        Reply {
            status: StatusCode::OK,
            link: None,
            body: body.to_owned(),
        }
    }

    fn client_for(base: &str) -> GithubRestClient {
        GithubRestClient::new("token", 20, base).expect("client")
    }

    fn bearer_of(headers: &HeaderMap) -> String {
        headers
            .get(reqwest::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_owned()
    }

    #[tokio::test]
    async fn get_file_roundtrip() {
        use base64::Engine as _;
        let content = base64_engine().encode(b"hello\nworld");
        // Newlines in the payload are discarded, as `validate=False` does
        // (GitHub wraps the base64 at 60 columns).
        let wrapped = format!("{}\\n{}", &content[..8], &content[8..]);
        let (base, state) = mock_server(vec![json_reply(&format!(
            r#"{{"type": "file", "path": "a/b.txt", "sha": "abc", "content": "{wrapped}"}}"#
        ))])
        .await;
        let client = client_for(&base);
        let file = client
            .get_file("o", "r", "a/b.txt", "main")
            .await
            .expect("file");
        assert_eq!(file["path"], Value::String("a/b.txt".to_owned()));
        assert_eq!(file["sha"], Value::String("abc".to_owned()));
        assert_eq!(file["content"], Value::String("hello\nworld".to_owned()));
        assert!(file.get("unsupported_content").is_none());
        let headers = state.headers.lock().unwrap();
        assert_eq!(bearer_of(&headers[0]), "Bearer token");
        assert_eq!(
            headers[0]
                .get(reqwest::header::USER_AGENT)
                .and_then(|value| value.to_str().ok()),
            Some(CLIENT_USER_AGENT)
        );
        let urls = state.urls.lock().unwrap();
        assert!(
            urls[0].ends_with("/repos/o/r/contents/a/b.txt?ref=main"),
            "{urls:?}"
        );
    }

    #[tokio::test]
    async fn get_file_ref_encoding_and_empty_ref() {
        let (base, state) = mock_server(vec![
            json_reply(r#"{"type": "file", "path": "p", "sha": "s", "content": ""}"#),
            json_reply(r#"{"type": "file", "path": "p", "sha": "s", "content": ""}"#),
        ])
        .await;
        let client = client_for(&base);
        client
            .get_file("o", "r", "p", "feature/x y")
            .await
            .expect("file");
        client.get_file("o", "r", "p", "").await.expect("file");
        let urls = state.urls.lock().unwrap();
        assert!(urls[0].ends_with("?ref=feature%2Fx+y"), "{urls:?}");
        assert!(!urls[1].contains('?'), "{urls:?}");
    }

    #[tokio::test]
    async fn get_file_control_dot_path_stays_bound() {
        // PIDASHCONV-1486: each `.\n.` / `.\t.` segment passes the
        // `..` guard, but the `url` crate strips tab/LF/CR and then
        // resolves the resulting `..` segments out of the bound
        // repo. requests/urllib3 instead percent-encodes the controls
        // and never creates dot segments, so the request stays under
        // `/repos/{owner}/{name}/contents/` (GitHub answers 404).
        let (base, state) = mock_server(vec![
            json_reply(r#"{"type": "file", "path": "p", "sha": "s", "content": ""}"#),
            json_reply(r#"{"type": "file", "path": "p", "sha": "s", "content": ""}"#),
            json_reply(r#"{"type": "file", "path": "p", "sha": "s", "content": ""}"#),
        ])
        .await;
        let client = client_for(&base);
        client
            .get_file(
                "o",
                "r",
                ".\n./.\n./.\n./.\n./repos/o2/r2/contents/secret",
                "",
            )
            .await
            .expect("file");
        client
            .get_file("o", "r", ".\t./.\t./.\t./.\t./orgs/x", "")
            .await
            .expect("file");
        client
            .get_file("o", "r", "a\x01b\x7fc\r", "")
            .await
            .expect("file");
        let urls = state.urls.lock().unwrap();
        for url in urls.iter() {
            assert!(
                url.starts_with("/repos/o/r/contents/"),
                "escaped the bound repo: {urls:?}"
            );
        }
        assert_eq!(
            urls[0], "/repos/o/r/contents/.%0A./.%0A./.%0A./.%0A./repos/o2/r2/contents/secret",
            "{urls:?}"
        );
        assert!(urls[1].contains("%09"), "{urls:?}");
        assert!(
            urls[2].contains("%01") && urls[2].contains("%7F") && urls[2].contains("%0D"),
            "{urls:?}"
        );
    }

    #[tokio::test]
    async fn get_file_rejects_non_file_and_oversize_and_binary() {
        use base64::Engine as _;
        let big = base64_engine().encode(vec![b'a'; FILE_RESULT_LIMIT_BYTES + 1]);
        let binary = base64_engine().encode([0xff, 0xfe, 0x00]);
        let (base, _) = mock_server(vec![
            json_reply(r#"{"type": "dir", "path": "d", "sha": "s", "content": ""}"#),
            json_reply(&format!(
                r#"{{"type": "file", "path": "p", "sha": "s", "content": "{big}"}}"#
            )),
            json_reply(&format!(
                r#"{{"type": "file", "path": null, "sha": null, "content": "{binary}"}}"#
            )),
        ])
        .await;
        let client = client_for(&base);
        let error = client.get_file("o", "r", "d", "").await.expect_err("dir");
        assert_eq!(error.python_type, "ValueError");
        assert_eq!(error.message, "path is not a file");
        let error = client.get_file("o", "r", "p", "").await.expect_err("big");
        assert_eq!(error.python_type, "ValueError");
        assert_eq!(error.message, "file exceeds Cloud Agent result limit");
        let file = client.get_file("o", "r", "p", "").await.expect("binary");
        assert_eq!(file["content"], Value::String(String::new()));
        assert_eq!(
            file["unsupported_content"],
            Value::String("binary".to_owned())
        );
        assert_eq!(file["path"], Value::Null);
    }

    #[tokio::test]
    async fn status_errors_stay_typed_with_body_text() {
        let (base, _) = mock_server(vec![
            Reply {
                status: StatusCode::UNAUTHORIZED,
                link: None,
                body: "bad credentials".to_owned(),
            },
            Reply {
                status: StatusCode::FORBIDDEN,
                link: None,
                body: "rate limited".to_owned(),
            },
            Reply {
                status: StatusCode::NOT_FOUND,
                link: None,
                body: "nope".to_owned(),
            },
            Reply {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                link: None,
                body: "boom".to_owned(),
            },
        ])
        .await;
        let client = client_for(&base);
        let error = client.get_pull_request("o", "r", 1).await.expect_err("401");
        assert_eq!(
            (error.python_type, error.message.as_str()),
            ("GithubAuthError", "bad credentials")
        );
        let error = client.get_pull_request("o", "r", 1).await.expect_err("403");
        assert_eq!(
            (error.python_type, error.message.as_str()),
            ("GithubPermissionError", "rate limited")
        );
        let error = client.get_pull_request("o", "r", 1).await.expect_err("404");
        assert_eq!(
            (error.python_type, error.message.as_str()),
            ("GithubNotFoundError", "nope")
        );
        let error = client.get_pull_request("o", "r", 1).await.expect_err("500");
        assert_eq!(error.python_type, "HTTPError");
        // `requests.HTTPError` (`models.py:1017-1025`): no body.
        assert_eq!(
            error.message,
            format!("500 Server Error: Internal Server Error for url: {base}/repos/o/r/pulls/1")
        );
    }

    #[test]
    fn diff_truncation_mirrors_encode_round_trip() {
        // `raw = response.text.encode()[:65536]`
        // (`github_client.py:197-198`): the cut lands on re-encoded
        // bytes, and `truncated` compares against the raw cut — so a
        // 64 KiB body with invalid bytes loses a char yet reports
        // `truncated: false`.
        let out = truncate_diff(b"abc");
        assert_eq!(out["diff"], Value::String("abc".to_owned()));
        assert_eq!(out["truncated"], Value::Bool(false));
        let out = truncate_diff(&vec![b'a'; 70_000]);
        assert_eq!(out["diff"].as_str().unwrap().len(), 65_536);
        assert_eq!(out["truncated"], Value::Bool(true));
        let mut content = vec![b'a'; 65_534];
        content.extend_from_slice(&[0xff, 0xff]);
        let out = truncate_diff(&content);
        assert_eq!(
            out["diff"],
            Value::String(format!("{}\u{fffd}", "a".repeat(65_534)))
        );
        assert_eq!(out["truncated"], Value::Bool(false));
        let out = truncate_diff(&vec![0xff; 70_000]);
        assert_eq!(out["diff"], Value::String("\u{fffd}".repeat(21_846)));
        assert_eq!(out["truncated"], Value::Bool(true));
    }

    #[test]
    fn http_error_names_client_and_server() {
        assert_eq!(
            GithubFailure::http_error(
                reqwest::StatusCode::UNPROCESSABLE_ENTITY,
                "ignored body",
                "https://x/y"
            )
            .message,
            "422 Client Error: Unprocessable Entity for url: https://x/y"
        );
    }

    #[tokio::test]
    async fn empty_token_rejected() {
        let error = GithubRestClient::new("", 20, "https://api.github.com").expect_err("empty");
        assert_eq!(error.python_type, "GithubAuthError");
        assert_eq!(error.message, "empty token");
    }

    #[tokio::test]
    async fn diff_truncates_and_flags() {
        let body = "x".repeat(DIFF_RESULT_LIMIT_BYTES + 10);
        let (base, state) = mock_server(vec![Reply {
            status: StatusCode::OK,
            link: None,
            body: body.clone(),
        }])
        .await;
        let client = client_for(&base);
        let diff = client
            .get_pull_request_diff("o", "r", 7)
            .await
            .expect("diff");
        assert_eq!(
            diff["diff"].as_str().unwrap().len(),
            DIFF_RESULT_LIMIT_BYTES
        );
        assert_eq!(diff["truncated"], Value::Bool(true));
        let headers = state.headers.lock().unwrap();
        assert_eq!(
            headers[0]
                .get(reqwest::header::ACCEPT)
                .and_then(|value| value.to_str().ok()),
            Some("application/vnd.github.diff")
        );
    }

    #[tokio::test]
    async fn files_cap_at_300() {
        let many: String = (0..301).map(|n| format!(r#"{{"n": {n}}},"#)).collect();
        let (base, _) = mock_server(vec![json_reply(&format!(
            "[{}]",
            many.trim_end_matches(',')
        ))])
        .await;
        let client = client_for(&base);
        let files = client
            .list_pull_request_files("o", "r", 1)
            .await
            .expect("files");
        assert_eq!(files.len(), PR_FILES_LIMIT);
        assert_eq!(files[0]["n"], Value::Number(0.into()));
    }

    #[tokio::test]
    async fn files_follow_next_links() {
        let (base, state) = mock_server(vec![]).await;
        let next = format!("{base}/repos/o/r/pulls/1/files?page=2");
        *state.replies.lock().unwrap() = vec![
            Reply {
                status: StatusCode::OK,
                link: Some(format!(r#"<{next}>; rel="next""#)),
                body: r#"[{"n": 1}]"#.to_owned(),
            },
            json_reply(r#"[{"n": 2}]"#),
        ];
        let client = client_for(&base);
        let files = client
            .list_pull_request_files("o", "r", 1)
            .await
            .expect("files");
        assert_eq!(files.len(), 2);
        assert_eq!(files[1]["n"], Value::Number(2.into()));
        let urls = state.urls.lock().unwrap();
        assert_eq!(urls.len(), 2);
        assert!(urls[0].contains("per_page=100"), "{urls:?}");
        assert!(urls[1].ends_with("?page=2"), "{urls:?}");
    }

    #[tokio::test]
    async fn checks_resolve_head_sha() {
        let (base, state) = mock_server(vec![
            json_reply(r#"{"head": {"sha": "deadbee"}}"#),
            json_reply(r#"{"total_count": 0}"#),
        ])
        .await;
        let client = client_for(&base);
        let checks = client
            .list_pull_request_checks("o", "r", 3)
            .await
            .expect("checks");
        assert_eq!(checks["total_count"], Value::Number(0.into()));
        let urls = state.urls.lock().unwrap();
        assert!(
            urls[1].ends_with("/repos/o/r/commits/deadbee/check-runs"),
            "{urls:?}"
        );
    }

    #[tokio::test]
    async fn checks_missing_head_is_key_error() {
        let (base, _) = mock_server(vec![json_reply(r#"{"head": {}}"#)]).await;
        let client = client_for(&base);
        let error = client
            .list_pull_request_checks("o", "r", 3)
            .await
            .expect_err("sha");
        assert_eq!(error.python_type, "KeyError");
        assert_eq!(error.message, "'sha'");
    }

    #[tokio::test]
    async fn checks_null_head_is_type_error() {
        // `None["sha"]`: `TypeError`, not `KeyError('sha')`.
        let (base, _) = mock_server(vec![json_reply(r#"{"head": null}"#)]).await;
        let client = client_for(&base);
        let error = client
            .list_pull_request_checks("o", "r", 3)
            .await
            .expect_err("null head");
        assert_eq!(error.python_type, "TypeError");
        assert_eq!(error.message, "'NoneType' object is not subscriptable");
    }

    #[tokio::test]
    async fn checks_non_string_sha_formats_into_url() {
        // An f-string formats anything: no raise.
        let (base, state) = mock_server(vec![
            json_reply(r#"{"head": {"sha": 123}}"#),
            json_reply(r#"{"total_count": 0}"#),
        ])
        .await;
        let client = client_for(&base);
        client
            .list_pull_request_checks("o", "r", 3)
            .await
            .expect("checks");
        let urls = state.urls.lock().unwrap();
        assert!(
            urls[1].ends_with("/repos/o/r/commits/123/check-runs"),
            "{urls:?}"
        );
    }

    #[test]
    fn next_url_parsing() {
        assert_eq!(
            GithubRestClient::next_url(r#"<https://a/b?page=2>; rel="next""#).as_deref(),
            Some("https://a/b?page=2")
        );
        assert_eq!(
            GithubRestClient::next_url(
                r#"<https://a/b?page=1>; rel="prev", <https://a/b?page=3>; rel="next""#
            )
            .as_deref(),
            Some("https://a/b?page=3")
        );
        // `re.match` semantics: trailing attributes after rel="next" match.
        assert_eq!(
            GithubRestClient::next_url(r#"<https://a/b>; rel="next"; title="x""#).as_deref(),
            Some("https://a/b")
        );
        assert_eq!(GithubRestClient::next_url(""), None);
        assert_eq!(
            GithubRestClient::next_url(r#"<https://a/b>; rel="last""#),
            None
        );
    }

    #[test]
    fn github_headers_values_and_bad_token() {
        // The four `_headers` values (github_client.py:54-60).
        let headers = github_headers("tok").expect("headers");
        let value = |name: reqwest::header::HeaderName| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .expect("value")
                .to_owned()
        };
        assert_eq!(
            value(reqwest::header::AUTHORIZATION),
            format!("Bearer {}", "tok")
        );
        assert_eq!(
            value(reqwest::header::ACCEPT),
            "application/vnd.github+json"
        );
        assert_eq!(
            headers
                .get("x-github-api-version")
                .and_then(|v| v.to_str().ok()),
            Some("2022-11-28")
        );
        assert_eq!(value(reqwest::header::USER_AGENT), CLIENT_USER_AGENT);
        // A token with control bytes is InvalidHeader, never a panic.
        let error = github_headers("bad\ntoken").expect_err("bad token");
        assert_eq!(error.message, "github bearer token");
        assert_eq!(error.to_string(), "github bearer token");
        let client =
            GithubRestClient::new("bad\ntoken", 30, "https://api.github.com").expect("client");
        let failure = client.headers().expect_err("bad token headers");
        assert_eq!(failure.python_type, "InvalidHeader");
        assert_eq!(failure.message, "github bearer token");
    }

    #[test]
    fn quote_plus_encoding() {
        assert_eq!(quote_plus("main"), "main");
        assert_eq!(quote_plus("feature/x y"), "feature%2Fx+y");
        assert_eq!(quote_plus("a~b_c.d-e"), "a~b_c.d-e");
        assert_eq!(quote_plus("caf\u{e9}"), "caf%C3%A9");
        assert_eq!(quote_plus("a+b"), "a%2Bb");
    }

    #[test]
    fn bad_pem_is_value_error() {
        let error = build_app_jwt("123", "not-a-key").expect_err("pem");
        assert_eq!(error.python_type, "ValueError");
    }

    #[test]
    fn token_cache_key_shape() {
        assert_eq!(
            installation_token_cache_key(42),
            "github_app_installation_token:42"
        );
    }
}
