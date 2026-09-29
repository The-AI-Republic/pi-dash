//! OAuth provider shapes: auth-url builders + token/user-data mappers (D-17).
//!
//! Port of `apps/api/pi_dash/authentication/provider/oauth/`:
//!
//! * `google.py:22-25` (consts), `:27-74` (`__init__`), `:76-101`
//!   (`set_token_data`), `:103-115` (`set_user_data`);
//! * `github.py:24-31` (urls + scope), `:33-82` (`__init__`), `:84-106`
//!   (`set_token_data`), `:108-135` (`__get_email`), `:137-143` (org gate),
//!   `:145-182` (`set_user_data`);
//! * `gitlab.py:22-23`, `:25-77` (`__init__`), `:79-107` (`set_token_data`),
//!   `:109-124` (`set_user_data`);
//! * `gitea.py:21-22`, `:24-86` (`__init__`), `:88-114` (`set_token_data`),
//!   `:116-147` (`__get_email`), `:149-173` (`set_user_data`).
//!
//! Fixture ids: AUTHOAUTH-F3 (auth-url goldens), AUTHOAUTH-F4 (token/user-data
//! mappers) under `rust-api/fixtures/auth_oauth/`.
//!
//! Only the pure shapes live here: URL building, `urlencode` ordering, the
//! stored token/user mappings, the email selectors. The HTTP exchange
//! (`get_user_token`, `get_user_response`, the `requests.get` calls inside
//! `__get_email` / the org gate) and the account upsert stay in the adapter
//! queries issue; the views stay in the handlers issues.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-3: gitlab `access_token_expired_at` sums `created_at + expires_in`
//!   with no `created_at` guard — `TypeError` when absent
//!   (`gitlab.py:79-107`). Ported as [`GitlabExpiryError`]: a truthy
//!   `expires_in` with missing/null/non-numeric `created_at` is an error,
//!   not a fallback.
//! * BUG-4: the fixture records github `self.scope += ' read:org'` as a
//!   class-level mutation leaking into later instances (`github.py:59-60`).
//!   Verified against CPython semantics 2026-09-29: `+=` on an immutable
//!   `str` rebinds an *instance* attribute, so the class attribute — and
//!   every later instance — is unaffected (no leak). [`github_scope`]
//!   therefore ports the observable behavior (org set → extended scope for
//!   that instance) as a pure function with no shared state; introducing a
//!   global to "reproduce" the leak would invent a bug Python does not have.

use chrono::{DateTime, TimeZone, Utc};
use serde::Serialize;
use serde_json::Value;

// ---------------------------------------------------------------------------
// urlencode (CPython urllib.parse.urlencode scalar encoding)
// ---------------------------------------------------------------------------

/// Encode one query component the way CPython `urllib.parse.quote_plus`
/// (used by `urlencode`) does: UTF-8 bytes, `A-Za-z0-9` plus `-_.~` kept
/// verbatim, space → `+`, every other byte → `%XX` uppercase.
pub fn quote_plus(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

/// `urllib.parse.urlencode` over an ordered pair list: insertion order is
/// preserved (each provider pins its `url_params_order` in the F3 fixture).
pub fn urlencode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", quote_plus(k), quote_plus(v)))
        .collect::<Vec<_>>()
        .join("&")
}

// ---------------------------------------------------------------------------
// Shared small helpers mirroring Python semantics
// ---------------------------------------------------------------------------

/// Python truthiness over JSON values (Semantic-traps checklist): `None` /
/// `false` / `0` / `""` / empty containers are falsy; everything else truthy.
pub fn json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `dict.get(key)` with a `None`→`Null` default, as the `set_user_data`
/// mappers read every userinfo field.
fn lookup(value: &Value, key: &str) -> Value {
    value.get(key).cloned().unwrap_or(Value::Null)
}

/// `full_name or login`: first truthy side wins, else `Null`.
fn or_lookup(first: Option<&Value>, second: Option<&Value>) -> Value {
    for candidate in [first, second].into_iter().flatten() {
        if json_truthy(candidate) {
            return candidate.clone();
        }
    }
    Value::Null
}

/// `datetime.fromtimestamp(x, tz=pytz.utc)` for a JSON number, guarded by
/// the caller's truthiness check (`if resp.get(...)` — `0` stays `None`).
/// Non-numeric input (absent key, `null`, strings) yields `None`; the gitlab
/// `created_at` case is the exception and has its own fallible mapper.
fn fromtimestamp_value(value: &Value) -> Option<DateTime<Utc>> {
    let as_number = match value {
        Value::Number(n) => n.as_f64(),
        Value::Bool(true) => Some(1.0),
        _ => None,
    }?;
    if as_number == 0.0 {
        return None;
    }
    let secs = as_number.trunc() as i64;
    let nanos = (as_number.fract().abs() * 1e9).round() as u32;
    Utc.timestamp_opt(secs, nanos).single()
}

/// `redirect_uri`: `'https' if request.is_secure() else 'http'` +
/// `://{host}/auth/<provider>/callback/`.
pub fn redirect_uri(is_secure: bool, host: &str, provider: &str) -> String {
    let scheme = if is_secure { "https" } else { "http" };
    format!("{scheme}://{host}/auth/{provider}/callback/")
}

// ---------------------------------------------------------------------------
// Provider constants (class attributes)
// ---------------------------------------------------------------------------

pub const GOOGLE_PROVIDER: &str = "google";
pub const GOOGLE_SCOPE: &str = "https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile";
pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
pub const GOOGLE_AUTH_URL_BASE: &str = "https://accounts.google.com/o/oauth2/v2/auth";

pub const GITHUB_PROVIDER: &str = "github";
pub const GITHUB_SCOPE: &str = "read:user user:email";
pub const GITHUB_ORGANIZATION_SCOPE: &str = "read:org";
pub const GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
pub const GITHUB_USERINFO_URL: &str = "https://api.github.com/user";
pub const GITHUB_EMAILS_URL: &str = "https://api.github.com/user/emails";
pub const GITHUB_ORG_MEMBERSHIP_URL: &str = "https://api.github.com/orgs";
pub const GITHUB_AUTH_URL_BASE: &str = "https://github.com/login/oauth/authorize";

pub const GITLAB_PROVIDER: &str = "gitlab";
pub const GITLAB_SCOPE: &str = "read_user";
pub const GITLAB_HOST_DEFAULT: &str = "https://gitlab.com";

pub const GITEA_PROVIDER: &str = "gitea";
pub const GITEA_SCOPE: &str = "openid email profile";

/// `Accept: application/json` POST headers used by the github/gitlab/gitea
/// token exchange (`headers={"Accept": "application/json"}`); google posts
/// with no headers (`{}`).
pub const TOKEN_JSON_ACCEPT_HEADERS: [(&str, &str); 1] = [("Accept", "application/json")];

// ---------------------------------------------------------------------------
// Configured checks (the `__init__` NOT_CONFIGURED guards)
// ---------------------------------------------------------------------------

/// `if not (GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET)` — either missing or
/// empty raises. Empty-string-is-falsy is the Semantic-traps item.
pub fn google_configured(client_id: Option<&str>, client_secret: Option<&str>) -> bool {
    client_id.is_some_and(|s| !s.is_empty()) && client_secret.is_some_and(|s| !s.is_empty())
}

pub fn github_configured(client_id: Option<&str>, client_secret: Option<&str>) -> bool {
    google_configured(client_id, client_secret)
}

pub fn gitlab_configured(
    client_id: Option<&str>,
    client_secret: Option<&str>,
    host: Option<&str>,
) -> bool {
    google_configured(client_id, client_secret) && host.is_some_and(|s| !s.is_empty())
}

/// `os.environ.get('GITLAB_HOST', 'https://gitlab.com')` evaluated as the
/// `get_configuration_value` default: the env value wins when set (even when
/// set-but-empty — the falsy host then fails the configured check above).
pub fn gitlab_host_default(env_host: Option<&str>) -> &str {
    env_host.unwrap_or(GITLAB_HOST_DEFAULT)
}

pub fn gitea_configured(
    client_id: Option<&str>,
    client_secret: Option<&str>,
    host: Option<&str>,
) -> bool {
    gitlab_configured(client_id, client_secret, host)
}

/// Gitea host normalization (`gitea.py:52-57`): a missing scheme or a scheme
/// outside `(https, http)` raises `GITEA_NOT_CONFIGURED` with no detail leak,
/// then trailing slashes are stripped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GiteaHostError {
    NotConfigured,
}

pub fn normalize_gitea_host(host: &str) -> Result<String, GiteaHostError> {
    let scheme_end = host.find("://").ok_or(GiteaHostError::NotConfigured)?;
    let scheme = &host[..scheme_end];
    if scheme != "https" && scheme != "http" {
        return Err(GiteaHostError::NotConfigured);
    }
    Ok(host.trim_end_matches('/').to_owned())
}

/// Effective github scope for one instance (`github.py:59-60`): the org scope
/// is appended only for that instance when an organization id is set. See
/// the BUG-4 note in the module docs — no shared state is involved.
pub fn github_scope(organization_id: Option<&str>) -> String {
    match organization_id {
        Some(org) if !org.is_empty() => format!("{GITHUB_SCOPE} {GITHUB_ORGANIZATION_SCOPE}"),
        _ => GITHUB_SCOPE.to_owned(),
    }
}

// ---------------------------------------------------------------------------
// auth_url builders (the `url_params` dicts + `urlencode` call per provider)
// ---------------------------------------------------------------------------

pub fn google_auth_url(client_id: &str, is_secure: bool, host: &str, state: &str) -> String {
    let uri = redirect_uri(is_secure, host, GOOGLE_PROVIDER);
    format!(
        "{}?{}",
        GOOGLE_AUTH_URL_BASE,
        urlencode(&[
            ("client_id", client_id),
            ("scope", GOOGLE_SCOPE),
            ("redirect_uri", &uri),
            ("response_type", "code"),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("state", state),
        ])
    )
}

pub fn github_auth_url(
    client_id: &str,
    is_secure: bool,
    host: &str,
    state: &str,
    organization_id: Option<&str>,
) -> String {
    let uri = redirect_uri(is_secure, host, GITHUB_PROVIDER);
    let scope = github_scope(organization_id);
    format!(
        "{}?{}",
        GITHUB_AUTH_URL_BASE,
        urlencode(&[
            ("client_id", client_id),
            ("redirect_uri", &uri),
            ("scope", &scope),
            ("state", state),
        ])
    )
}

pub fn gitlab_auth_url(
    client_id: &str,
    is_secure: bool,
    host: &str,
    state: &str,
    gitlab_host: &str,
) -> String {
    let uri = redirect_uri(is_secure, host, GITLAB_PROVIDER);
    format!(
        "{gitlab_host}/oauth/authorize?{}",
        urlencode(&[
            ("client_id", client_id),
            ("redirect_uri", &uri),
            ("response_type", "code"),
            ("scope", GITLAB_SCOPE),
            ("state", state),
        ])
    )
}

pub fn gitlab_token_url(gitlab_host: &str) -> String {
    format!("{gitlab_host}/oauth/token")
}

pub fn gitlab_userinfo_url(gitlab_host: &str) -> String {
    format!("{gitlab_host}/api/v4/user")
}

pub fn gitea_auth_url(
    client_id: &str,
    is_secure: bool,
    host: &str,
    state: &str,
    gitea_host_normalized: &str,
) -> String {
    let uri = redirect_uri(is_secure, host, GITEA_PROVIDER);
    format!(
        "{gitea_host_normalized}/login/oauth/authorize?{}",
        urlencode(&[
            ("client_id", client_id),
            ("scope", GITEA_SCOPE),
            ("redirect_uri", &uri),
            ("response_type", "code"),
            ("state", state),
        ])
    )
}

pub fn gitea_token_url(gitea_host_normalized: &str) -> String {
    format!("{gitea_host_normalized}/login/oauth/access_token")
}

pub fn gitea_userinfo_url(gitea_host_normalized: &str) -> String {
    format!("{gitea_host_normalized}/api/v1/user")
}

pub fn gitea_emails_url(userinfo_url: &str) -> String {
    format!("{userinfo_url}/emails")
}

/// `is_user_in_organization`: member iff the membership GET returns 200
/// (`github.py:137-143`).
pub fn is_org_member(status_code: u16) -> bool {
    status_code == 200
}

pub fn github_membership_url(
    org_membership_base: &str,
    organization_id: &str,
    login: &str,
) -> String {
    format!("{org_membership_base}/{organization_id}/memberships/{login}")
}

// ---------------------------------------------------------------------------
// Token POST bodies (the `data` dicts in key order; headers above)
// ---------------------------------------------------------------------------

/// google `set_token_data` POST body (`google.py:76-83`).
pub fn google_token_post_data(
    code: &str,
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
) -> Vec<(&'static str, String)> {
    vec![
        ("code", code.to_owned()),
        ("client_id", client_id.to_owned()),
        ("client_secret", client_secret.to_owned()),
        ("redirect_uri", redirect_uri.to_owned()),
        ("grant_type", "authorization_code".to_owned()),
    ]
}

/// github `set_token_data` POST body (`github.py:84-89`).
pub fn github_token_post_data(
    code: &str,
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
) -> Vec<(&'static str, String)> {
    vec![
        ("client_id", client_id.to_owned()),
        ("client_secret", client_secret.to_owned()),
        ("code", code.to_owned()),
        ("redirect_uri", redirect_uri.to_owned()),
    ]
}

/// gitlab/gitea `set_token_data` POST body (`gitlab.py:79-85`,
/// `gitea.py:88-94` — identical key order).
pub fn authorization_code_post_data(
    code: &str,
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
) -> Vec<(&'static str, String)> {
    vec![
        ("client_id", client_id.to_owned()),
        ("client_secret", client_secret.to_owned()),
        ("code", code.to_owned()),
        ("redirect_uri", redirect_uri.to_owned()),
        ("grant_type", "authorization_code".to_owned()),
    ]
}

/// gitea `set_token_data` POST body (`gitea.py:88-94`): same keys as the
/// shared authorization-code body in a different order.
pub fn gitea_token_post_data(
    code: &str,
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
) -> Vec<(&'static str, String)> {
    vec![
        ("code", code.to_owned()),
        ("client_id", client_id.to_owned()),
        ("client_secret", client_secret.to_owned()),
        ("redirect_uri", redirect_uri.to_owned()),
        ("grant_type", "authorization_code".to_owned()),
    ]
}

// ---------------------------------------------------------------------------
// Stored token data (the `super().set_token_data({...})` dicts)
// ---------------------------------------------------------------------------

/// The five stored token fields, in the exact key order every provider's
/// `set_token_data` uses (`access_token`, `refresh_token`,
/// `access_token_expired_at`, `refresh_token_expired_at`, `id_token`).
/// String-ish fields pass the response through verbatim (`Value`, `Null`
/// when absent); `id_token` defaults to `""`, `refresh_token` to `Null`,
/// mirroring `.get("id_token", "")` / `.get("refresh_token", None)`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TokenData {
    pub access_token: Value,
    pub refresh_token: Value,
    pub access_token_expired_at: Option<DateTime<Utc>>,
    pub refresh_token_expired_at: Option<DateTime<Utc>>,
    pub id_token: Value,
}

fn base_token_data(token_response: &Value) -> TokenData {
    TokenData {
        access_token: lookup(token_response, "access_token"),
        refresh_token: lookup(token_response, "refresh_token"),
        access_token_expired_at: None,
        refresh_token_expired_at: fromtimestamp_value(
            token_response
                .get("refresh_token_expired_at")
                .unwrap_or(&Value::Null),
        ),
        id_token: token_response
            .get("id_token")
            .cloned()
            .unwrap_or(Value::String(String::new())),
    }
}

/// google/github `set_token_data` expiry basis (`google.py:86-99`,
/// `github.py:91-104`): `fromtimestamp(expires_in)` — an absolute epoch.
fn epoch_expiry(token_response: &Value) -> Option<DateTime<Utc>> {
    token_response
        .get("expires_in")
        .and_then(fromtimestamp_value)
}

/// google `set_token_data` (`google.py:76-101`).
pub fn google_token_data(token_response: &Value) -> TokenData {
    let mut stored = base_token_data(token_response);
    stored.access_token_expired_at = epoch_expiry(token_response);
    stored
}

/// github `set_token_data` (`github.py:84-106`): same expiry basis as google.
pub fn github_token_data(token_response: &Value) -> TokenData {
    google_token_data(token_response)
}

/// BUG-3: gitlab sums `created_at + expires_in` with no `created_at` guard
/// (`gitlab.py:88-95`) — absent/null/non-numeric `created_at` alongside a
/// truthy `expires_in` raised `TypeError` in Python and is an error here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitlabExpiryError {
    MissingCreatedAt,
}

/// gitlab `set_token_data` (`gitlab.py:79-107`).
pub fn gitlab_token_data(token_response: &Value) -> Result<TokenData, GitlabExpiryError> {
    let mut stored = base_token_data(token_response);
    let expires_in = token_response.get("expires_in").unwrap_or(&Value::Null);
    if !json_truthy(expires_in) {
        return Ok(stored);
    }
    let created_at = token_response
        .get("created_at")
        .and_then(Value::as_f64)
        .ok_or(GitlabExpiryError::MissingCreatedAt)?;
    let expires_in = expires_in
        .as_f64()
        .ok_or(GitlabExpiryError::MissingCreatedAt)?;
    stored.access_token_expired_at = fromtimestamp_value(&Value::from(created_at + expires_in));
    Ok(stored)
}

/// gitea `set_token_data` (`gitea.py:88-114`): the access-token expiry uses
/// a WALL-CLOCK basis (`datetime.now(utc) + timedelta(seconds=expires_in)`),
/// unlike the google/github/gitlab `fromtimestamp` basis. `now` is a
/// parameter so tests pin it; callers pass `Utc::now()`.
pub fn gitea_token_data(token_response: &Value, now: DateTime<Utc>) -> TokenData {
    let mut stored = base_token_data(token_response);
    stored.access_token_expired_at = token_response
        .get("expires_in")
        .and_then(Value::as_f64)
        .filter(|f| *f != 0.0)
        .and_then(|secs| {
            let whole = secs.trunc() as i64;
            let nanos = (secs.fract().abs() * 1e9).round() as i64;
            now.checked_add_signed(
                chrono::Duration::seconds(whole) + chrono::Duration::nanoseconds(nanos),
            )
        });
    stored
}

// ---------------------------------------------------------------------------
// Email selectors (the `__get_email` privates)
// ---------------------------------------------------------------------------

/// github `__get_email` failure modes (`github.py:108-135`), both raised as
/// `GITHUB_OAUTH_PROVIDER_ERROR` in Python.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubEmailError {
    NotAList,
    NoPrimaryEmail,
}

/// First entry with a truthy `primary` flag. Entries lacking `primary` are
/// skipped via `.get` (their `KeyError` in Python never survived
/// `set_user_data` — no fixture pins it); a present-but-keyless `email`
/// reads as `Null`, mirroring `.get`-style access on the selected entry.
pub fn github_primary_email(emails: &Value) -> Result<Value, GithubEmailError> {
    let list = emails.as_array().ok_or(GithubEmailError::NotAList)?;
    for entry in list {
        if json_truthy(entry.get("primary").unwrap_or(&Value::Null)) {
            return Ok(entry.get("email").cloned().unwrap_or(Value::Null));
        }
    }
    Err(GithubEmailError::NoPrimaryEmail)
}

/// gitea `__get_email` (`gitea.py:116-147`): preference order
/// primary+verified → verified → primary → first element; empty list raises
/// (`GITEA_OAUTH_PROVIDER_ERROR: No emails found`). Entries use `.get`
/// access exactly as in Python.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GiteaEmailError {
    NoEmails,
}

pub fn gitea_preferred_email(emails: &[Value]) -> Result<Value, GiteaEmailError> {
    if emails.is_empty() {
        return Err(GiteaEmailError::NoEmails);
    }
    let pick = |pred: &dyn Fn(&Value) -> bool| {
        emails
            .iter()
            .find(|e| pred(e))
            .and_then(|e| e.get("email"))
            .cloned()
    };
    if let Some(email) = pick(&|e| {
        json_truthy(e.get("primary").unwrap_or(&Value::Null))
            && json_truthy(e.get("verified").unwrap_or(&Value::Null))
    }) {
        return Ok(email);
    }
    if let Some(email) = pick(&|e| json_truthy(e.get("verified").unwrap_or(&Value::Null))) {
        return Ok(email);
    }
    if let Some(email) = pick(&|e| json_truthy(e.get("primary").unwrap_or(&Value::Null))) {
        return Ok(email);
    }
    Ok(emails[0].get("email").cloned().unwrap_or(Value::Null))
}

// ---------------------------------------------------------------------------
// Stored user data (the `super().set_user_data({...})` dicts)
// ---------------------------------------------------------------------------

/// google `set_user_data` shape (`google.py:103-115`): note the inner dict
/// carries NO `email` key, unlike the other three providers. Field order is
/// the Python dict order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GoogleUserData {
    pub email: Value,
    pub user: GoogleUser,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GoogleUser {
    pub avatar: Value,
    pub first_name: Value,
    pub last_name: Value,
    pub provider_id: Value,
    pub is_password_autoset: bool,
}

/// github/gitlab/gitea `set_user_data` shape (`github.py:171-181`,
/// `gitlab.py:113-123`, `gitea.py:162-172`): identical key order in all
/// three Python dicts, so one struct serves them. Field order is the Python
/// dict order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProviderUserData {
    pub email: Value,
    pub user: ProviderUser,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProviderUser {
    pub provider_id: Value,
    pub email: Value,
    pub avatar: Value,
    pub first_name: Value,
    pub last_name: Value,
    pub is_password_autoset: bool,
}

/// google `set_user_data` (`google.py:103-115`).
pub fn google_user_data(userinfo: &Value) -> GoogleUserData {
    GoogleUserData {
        email: lookup(userinfo, "email"),
        user: GoogleUser {
            avatar: lookup(userinfo, "picture"),
            first_name: lookup(userinfo, "given_name"),
            last_name: lookup(userinfo, "family_name"),
            provider_id: lookup(userinfo, "id"),
            is_password_autoset: true,
        },
    }
}

/// github `set_user_data` (`github.py:145-182`): `email` is the
/// `__get_email` result (primary email), stored both top-level and inside
/// `user`; `first_name` is `userinfo.name`, `last_name` is
/// `userinfo.family_name`.
pub fn github_user_data(userinfo: &Value, email: Value) -> ProviderUserData {
    ProviderUserData {
        email: email.clone(),
        user: ProviderUser {
            provider_id: lookup(userinfo, "id"),
            email,
            avatar: lookup(userinfo, "avatar_url"),
            first_name: lookup(userinfo, "name"),
            last_name: lookup(userinfo, "family_name"),
            is_password_autoset: true,
        },
    }
}

/// gitlab `set_user_data` (`gitlab.py:109-124`): `email` straight from
/// userinfo (no separate fetch).
pub fn gitlab_user_data(userinfo: &Value) -> ProviderUserData {
    let email = lookup(userinfo, "email");
    ProviderUserData {
        email: email.clone(),
        user: ProviderUser {
            provider_id: lookup(userinfo, "id"),
            email,
            avatar: lookup(userinfo, "avatar_url"),
            first_name: lookup(userinfo, "name"),
            last_name: lookup(userinfo, "family_name"),
            is_password_autoset: true,
        },
    }
}

/// gitea `set_user_data` (`gitea.py:149-173`): `provider_id` is `str(id)`;
/// `first_name` is `full_name or login`; `last_name` is always `""`.
pub fn gitea_user_data(userinfo: &Value, email: Value) -> ProviderUserData {
    let id = lookup(userinfo, "id");
    let id_str = match &id {
        Value::String(s) => s.clone(),
        Value::Null => "None".to_owned(),
        // Python `str(True)` is `"True"`, not JSON `"true"`.
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        other => other.to_string(),
    };
    ProviderUserData {
        email: email.clone(),
        user: ProviderUser {
            provider_id: Value::String(id_str),
            email,
            avatar: lookup(userinfo, "avatar_url"),
            first_name: or_lookup(userinfo.get("full_name"), userinfo.get("login")),
            last_name: Value::String(String::new()),
            is_password_autoset: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture_f3() -> Value {
        let path = format!(
            "{}/../../fixtures/auth_oauth/F3_provider_auth_url.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    fn fixture_f4() -> Value {
        let path = format!(
            "{}/../../fixtures/auth_oauth/F4_provider_token_user_data.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    fn query_keys(url: &str) -> Vec<String> {
        url.split('?')
            .nth(1)
            .unwrap()
            .split('&')
            .map(|pair| pair.split('=').next().unwrap().to_owned())
            .collect()
    }

    // -- F3: golden URLs ----------------------------------------------------

    #[test]
    fn google_auth_url_matches_golden() {
        let g = &fixture_f3()["google"];
        let golden = g["golden_example"].clone();
        let is_secure = golden["scheme"].as_str().unwrap() == "https";
        let built = google_auth_url(
            golden["client_id"].as_str().unwrap(),
            is_secure,
            golden["host"].as_str().unwrap(),
            golden["state"].as_str().unwrap(),
        );
        assert_eq!(built, golden["url"].as_str().unwrap());
        assert_eq!(
            query_keys(&built),
            g["url_params_order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn github_auth_url_matches_golden() {
        let g = &fixture_f3()["github"];
        let golden = g["golden_example"].clone();
        assert!(golden["org_id"].is_null());
        let is_secure = golden["scheme"].as_str().unwrap() == "https";
        let built = github_auth_url(
            golden["client_id"].as_str().unwrap(),
            is_secure,
            golden["host"].as_str().unwrap(),
            golden["state"].as_str().unwrap(),
            None,
        );
        assert_eq!(built, golden["url"].as_str().unwrap());
        assert_eq!(
            query_keys(&built),
            g["url_params_order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        );
        // The recorded scope encoding inside the golden URL.
        assert!(built.contains(&format!(
            "scope={}",
            golden["scope_in_url"].as_str().unwrap()
        )));
    }

    #[test]
    fn gitlab_auth_url_matches_golden_with_host_default() {
        let g = &fixture_f3()["gitlab"];
        let golden = g["golden_example"].clone();
        assert!(golden["gitlab_host_cfg"].is_null());
        let resolved = gitlab_host_default(None);
        assert_eq!(resolved, golden["resolved_gitlab_host"].as_str().unwrap());
        let is_secure = golden["scheme"].as_str().unwrap() == "https";
        // The gitlab golden echoes no `state` field; the golden URL itself
        // carries `state=STATE123`.
        let built = gitlab_auth_url(
            golden["client_id"].as_str().unwrap(),
            is_secure,
            golden["host"].as_str().unwrap(),
            "STATE123",
            resolved,
        );
        assert_eq!(built, golden["url"].as_str().unwrap());
        assert_eq!(
            query_keys(&built),
            g["url_params_order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn gitea_auth_url_matches_golden_with_host_normalization() {
        let g = &fixture_f3()["gitea"];
        let golden = g["golden_example"].clone();
        let normalized = normalize_gitea_host(golden["gitea_host_cfg"].as_str().unwrap()).unwrap();
        assert_eq!(normalized, golden["normalized_host"].as_str().unwrap());
        let is_secure = golden["scheme"].as_str().unwrap() == "https";
        // The gitea golden echoes no `state` field; the golden URL itself
        // carries `state=STATE123`.
        let built = gitea_auth_url(
            golden["client_id"].as_str().unwrap(),
            is_secure,
            golden["host"].as_str().unwrap(),
            "STATE123",
            &normalized,
        );
        assert_eq!(built, golden["url"].as_str().unwrap());
        assert_eq!(
            query_keys(&built),
            g["url_params_order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn provider_consts_match_fixture() {
        let f3 = fixture_f3();
        assert_eq!(f3["google"]["scope"].as_str().unwrap(), GOOGLE_SCOPE);
        assert_eq!(
            f3["google"]["token_url"].as_str().unwrap(),
            GOOGLE_TOKEN_URL
        );
        assert_eq!(
            f3["google"]["userinfo_url"].as_str().unwrap(),
            GOOGLE_USERINFO_URL
        );
        assert_eq!(
            f3["github"]["scope_default"].as_str().unwrap(),
            GITHUB_SCOPE
        );
        assert_eq!(
            f3["github"]["token_url"].as_str().unwrap(),
            GITHUB_TOKEN_URL
        );
        assert_eq!(
            f3["github"]["userinfo_url"].as_str().unwrap(),
            GITHUB_USERINFO_URL
        );
        assert_eq!(f3["gitlab"]["scope"].as_str().unwrap(), GITLAB_SCOPE);
        assert_eq!(f3["gitea"]["scope"].as_str().unwrap(), GITEA_SCOPE);
        assert_eq!(
            f3["gitlab"]["gitlab_host_default"].as_str().unwrap(),
            "os.environ.get('GITLAB_HOST','https://gitlab.com') passed as \
             get_configuration_value default; falsy resolved HOST also fails \
             the configured-check"
        );
        assert_eq!(GITLAB_HOST_DEFAULT, "https://gitlab.com");
    }

    // -- urlencode / configured checks ---------------------------------------

    #[test]
    fn quote_plus_matches_cpython_scalars() {
        assert_eq!(
            quote_plus("read:user user:email"),
            "read%3Auser+user%3Aemail"
        );
        assert_eq!(
            quote_plus("http://app.example.com/auth/github/callback/"),
            "http%3A%2F%2Fapp.example.com%2Fauth%2Fgithub%2Fcallback%2F"
        );
        assert_eq!(quote_plus("a-_.~09AZaz"), "a-_.~09AZaz");
        assert_eq!(quote_plus("a+b&c=d"), "a%2Bb%26c%3Dd");
    }

    #[test]
    fn configured_checks_follow_python_truthiness() {
        assert!(google_configured(Some("id"), Some("secret")));
        assert!(!google_configured(None, Some("secret")));
        assert!(!google_configured(Some(""), Some("secret")));
        assert!(!github_configured(Some("id"), None));
        assert!(gitlab_configured(
            Some("i"),
            Some("s"),
            Some("https://gitlab.com")
        ));
        assert!(!gitlab_configured(Some("i"), Some("s"), Some("")));
        assert!(!gitlab_configured(Some("i"), Some("s"), None));
        assert!(gitea_configured(
            Some("i"),
            Some("s"),
            Some("https://g.example/")
        ));
        assert!(!gitea_configured(
            Some("i"),
            None,
            Some("https://g.example/")
        ));
    }

    #[test]
    fn gitea_host_scheme_check_leaks_no_detail() {
        assert_eq!(
            normalize_gitea_host("https://gitea.example.com///").unwrap(),
            "https://gitea.example.com"
        );
        // Missing scheme or a non-http(s) scheme → bare NOT_CONFIGURED.
        assert_eq!(
            normalize_gitea_host("gitea.example.com").unwrap_err(),
            GiteaHostError::NotConfigured
        );
        assert_eq!(
            normalize_gitea_host("ftp://gitea.example.com").unwrap_err(),
            GiteaHostError::NotConfigured
        );
        assert_eq!(
            normalize_gitea_host("").unwrap_err(),
            GiteaHostError::NotConfigured
        );
    }

    #[test]
    fn github_org_scope_is_per_instance() {
        assert_eq!(github_scope(None), "read:user user:email");
        assert_eq!(github_scope(Some("")), "read:user user:email");
        assert_eq!(github_scope(Some("123")), "read:user user:email read:org");
        // No class-level leak: a default call after an org call is unaffected.
        let _ = github_scope(Some("123"));
        assert_eq!(github_scope(None), "read:user user:email");
        let url = github_auth_url("CID", false, "h.example", "S", Some("123"));
        assert!(url.contains("scope=read%3Auser+user%3Aemail+read%3Aorg"));
    }

    // -- F4: token POST bodies -------------------------------------------------

    #[test]
    fn token_post_data_key_orders_match_fixture() {
        let f4 = fixture_f4();
        let keys =
            |pairs: &[(&str, String)]| pairs.iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>();
        let expected = |ptr: &str| {
            f4.pointer(ptr)
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            keys(&google_token_post_data("c", "i", "s", "r")),
            expected("/google/set_token_data/post_data_keys_order")
        );
        assert_eq!(
            keys(&github_token_post_data("c", "i", "s", "r")),
            expected("/github/set_token_data/post_data_keys_order")
        );
        assert_eq!(
            keys(&authorization_code_post_data("c", "i", "s", "r")),
            expected("/gitlab/set_token_data/post_data_keys_order")
        );
        assert_eq!(
            keys(&gitea_token_post_data("c", "i", "s", "r")),
            expected("/gitea/set_token_data/post_data_keys_order")
        );
        assert_eq!(TOKEN_JSON_ACCEPT_HEADERS, [("Accept", "application/json")]);
    }

    // -- F4: token mappers -----------------------------------------------------

    fn epoch(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).single().unwrap()
    }

    #[test]
    fn google_token_expiry_uses_fromtimestamp_basis() {
        let stored = google_token_data(&json!({
            "access_token": "AT", "refresh_token": "RT",
            "expires_in": 3600, "refresh_token_expired_at": 7200, "id_token": "ID",
        }));
        assert_eq!(stored.access_token, json!("AT"));
        assert_eq!(stored.refresh_token, json!("RT"));
        assert_eq!(stored.access_token_expired_at, Some(epoch(3600)));
        assert_eq!(stored.refresh_token_expired_at, Some(epoch(7200)));
        assert_eq!(stored.id_token, json!("ID"));
    }

    #[test]
    fn token_expiry_guard_is_truthiness_not_presence() {
        // `expires_in: 0` is falsy → None, exactly like Python.
        let stored = google_token_data(&json!({"access_token": "AT", "expires_in": 0}));
        assert_eq!(stored.access_token_expired_at, None);
        // Absent/None refresh fields take their `.get` defaults.
        assert_eq!(stored.refresh_token, Value::Null);
        assert_eq!(stored.refresh_token_expired_at, None);
        assert_eq!(stored.id_token, json!(""));
    }

    #[test]
    fn github_token_mapper_matches_google_basis() {
        let resp = json!({"access_token": "AT", "expires_in": 3600});
        assert_eq!(github_token_data(&resp), google_token_data(&resp));
    }

    #[test]
    fn gitlab_token_expiry_sums_created_at_plus_expires_in() {
        let stored = gitlab_token_data(&json!({
            "access_token": "AT", "created_at": 1_700_000_000, "expires_in": 7200,
        }))
        .unwrap();
        assert_eq!(stored.access_token_expired_at, Some(epoch(1_700_007_200)));
    }

    #[test]
    fn gitlab_missing_created_at_is_the_ported_type_error() {
        // BUG-3: truthy expires_in + absent created_at raised TypeError.
        for resp in [
            json!({"access_token": "AT", "expires_in": 7200}),
            json!({"access_token": "AT", "created_at": null, "expires_in": 7200}),
        ] {
            assert_eq!(
                gitlab_token_data(&resp).unwrap_err(),
                GitlabExpiryError::MissingCreatedAt
            );
        }
        // Falsy expires_in never touches created_at.
        let stored = gitlab_token_data(&json!({"access_token": "AT"})).unwrap();
        assert_eq!(stored.access_token_expired_at, None);
    }

    #[test]
    fn gitea_token_expiry_uses_wall_clock_basis() {
        let now = epoch(1_700_000_000);
        let stored = gitea_token_data(&json!({"access_token": "AT", "expires_in": 3600}), now);
        assert_eq!(
            stored.access_token_expired_at,
            now.checked_add_signed(chrono::Duration::seconds(3600))
        );
        // ...which differs from the fromtimestamp basis for the same input.
        assert_ne!(
            stored.access_token_expired_at,
            google_token_data(&json!({"expires_in": 3600})).access_token_expired_at
        );
        let absent = gitea_token_data(&json!({"access_token": "AT"}), now);
        assert_eq!(absent.access_token_expired_at, None);
    }

    // -- F4: email selectors ---------------------------------------------------

    #[test]
    fn github_primary_email_selects_first_truthy_primary() {
        let emails = json!([
            {"email": "second@example.com", "primary": false},
            {"email": "first@example.com", "primary": true},
        ]);
        assert_eq!(
            github_primary_email(&emails).unwrap(),
            json!("first@example.com")
        );
    }

    #[test]
    fn github_email_failures_map_to_provider_error() {
        // Non-list response → error (fixture: "response not a list").
        assert_eq!(
            github_primary_email(&json!({"email": "x@example.com"})).unwrap_err(),
            GithubEmailError::NotAList
        );
        // No primary email → error.
        assert_eq!(
            github_primary_email(&json!([{"email": "x@example.com", "primary": false}]))
                .unwrap_err(),
            GithubEmailError::NoPrimaryEmail
        );
        assert_eq!(
            github_primary_email(&json!([])).unwrap_err(),
            GithubEmailError::NoPrimaryEmail
        );
    }

    #[test]
    fn gitea_email_preference_order() {
        let only_verified = vec![
            json!({"email": "plain@example.com", "primary": true, "verified": false}),
            json!({"email": "verified@example.com", "primary": false, "verified": true}),
        ];
        assert_eq!(
            gitea_preferred_email(&only_verified).unwrap(),
            json!("verified@example.com")
        );
        let primary_plus_verified = vec![
            json!({"email": "v@example.com", "primary": false, "verified": true}),
            json!({"email": "pv@example.com", "primary": true, "verified": true}),
            json!({"email": "p@example.com", "primary": true, "verified": false}),
        ];
        assert_eq!(
            gitea_preferred_email(&primary_plus_verified).unwrap(),
            json!("pv@example.com")
        );
        // Falls back to the first element when nothing is flagged.
        let unflagged = vec![json!({"email": "first@example.com"})];
        assert_eq!(
            gitea_preferred_email(&unflagged).unwrap(),
            json!("first@example.com")
        );
        assert_eq!(
            gitea_preferred_email(&[]).unwrap_err(),
            GiteaEmailError::NoEmails
        );
    }

    #[test]
    fn github_org_gate_url_and_200_rule() {
        assert_eq!(
            github_membership_url(GITHUB_ORG_MEMBERSHIP_URL, "123", "octocat"),
            "https://api.github.com/orgs/123/memberships/octocat"
        );
        assert!(is_org_member(200));
        assert!(!is_org_member(404));
        assert!(!is_org_member(204));
    }

    // -- F4: user-data mappers --------------------------------------------------

    #[test]
    fn google_user_data_shape_and_bytes() {
        let mapped = google_user_data(&json!({
            "email": "u@example.com", "picture": "pic", "given_name": "G",
            "family_name": "F", "id": "google-id-1",
        }));
        assert_eq!(
            serde_json::to_string(&mapped).unwrap(),
            r#"{"email":"u@example.com","user":{"avatar":"pic","first_name":"G","last_name":"F","provider_id":"google-id-1","is_password_autoset":true}}"#
        );
        // Missing keys read as null (`.get` defaults).
        let sparse = google_user_data(&json!({}));
        assert_eq!(sparse.email, Value::Null);
        assert_eq!(sparse.user.provider_id, Value::Null);
    }

    #[test]
    fn github_user_data_carries_primary_email_twice() {
        let mapped = github_user_data(
            &json!({"id": 42, "avatar_url": "av", "name": "N", "family_name": "F"}),
            json!("p@example.com"),
        );
        assert_eq!(
            serde_json::to_string(&mapped).unwrap(),
            r#"{"email":"p@example.com","user":{"provider_id":42,"email":"p@example.com","avatar":"av","first_name":"N","last_name":"F","is_password_autoset":true}}"#
        );
    }

    #[test]
    fn gitlab_user_data_uses_userinfo_email() {
        let mapped = gitlab_user_data(&json!({
            "id": 7, "email": "g@example.com", "avatar_url": "av",
            "name": "N", "family_name": "F",
        }));
        assert_eq!(mapped.email, json!("g@example.com"));
        assert_eq!(mapped.user.email, json!("g@example.com"));
        assert_eq!(mapped.user.provider_id, json!(7));
    }

    #[test]
    fn gitea_user_data_str_id_full_name_or_login_and_blank_last() {
        let mapped = gitea_user_data(
            &json!({"id": 99, "avatar_url": "av", "login": "octocat"}),
            json!("gt@example.com"),
        );
        assert_eq!(mapped.user.provider_id, json!("99"));
        assert_eq!(mapped.user.first_name, json!("octocat"));
        assert_eq!(mapped.user.last_name, json!(""));
        let named = gitea_user_data(
            &json!({"id": 99, "full_name": "Full Name", "login": "octocat"}),
            json!("gt@example.com"),
        );
        assert_eq!(named.user.first_name, json!("Full Name"));
        // Empty full_name falls through to login (`or` semantics).
        let empty_named = gitea_user_data(
            &json!({"id": 99, "full_name": "", "login": "octocat"}),
            json!("gt@example.com"),
        );
        assert_eq!(empty_named.user.first_name, json!("octocat"));
        assert_eq!(
            serde_json::to_string(&mapped).unwrap(),
            r#"{"email":"gt@example.com","user":{"provider_id":"99","email":"gt@example.com","avatar":"av","first_name":"octocat","last_name":"","is_password_autoset":true}}"#
        );
    }

    #[test]
    fn gitea_emails_url_derives_from_userinfo_url() {
        assert_eq!(
            gitea_userinfo_url("https://gitea.example.com"),
            "https://gitea.example.com/api/v1/user"
        );
        assert_eq!(
            gitea_emails_url("https://gitea.example.com/api/v1/user"),
            "https://gitea.example.com/api/v1/user/emails"
        );
        assert_eq!(
            gitea_token_url("https://gitea.example.com"),
            "https://gitea.example.com/login/oauth/access_token"
        );
        assert_eq!(
            gitlab_token_url("https://gitlab.com"),
            "https://gitlab.com/oauth/token"
        );
        assert_eq!(
            gitlab_userinfo_url("https://gitlab.com"),
            "https://gitlab.com/api/v4/user"
        );
    }
}
