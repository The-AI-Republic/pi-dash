//! GitLab provider adapter (D-05, stage 5).
//!
//! Ports `apps/api/pi_dash/integrations/git/adapters/gitlab.py:37-412`: the
//! `_normalize_host` / `_allowed_hosts` / `_parse_dt` / `_strip_git_suffix` /
//! `_split_full_path` helpers, the blocking `GitLabClient`, and the
//! `GitLabAdapter` (key `"gitlab"`).
//!
//! Layering notes:
//!
//! * The services crate holds no HTTP client (network I/O lives in
//!   `pidash-api`, which owns `reqwest`), so the client runs against an
//!   injected [`GitLabTransport`]. The trait carries the exact
//!   `requests.request` contract the Python relies on: `timeout` and
//!   `allow_redirects=false` are structural (the client maps 3xx itself, so
//!   a transport MUST surface redirect statuses instead of following them),
//!   and query values use the `requests` spelling (`membership=True`, not
//!   `true`). Unit tests inject a recording fake; the real `reqwest`
//!   transport belongs to the later wiring issue that serves these methods.
//! * Instance settings (`GITLAB_HOST`, `GITLAB_ALLOWED_HOSTS`) cross as
//!   [`GitLabConfig`] (Django `settings` reads, `gitlab.py:49-62`); the
//!   per-credential `host_url` keeps priority over the configured host,
//!   exactly like `_client` (`gitlab.py:200-209`).
//! * Token decryption (`decrypt_data(token) or token`, `gitlab.py:191-198`)
//!   crosses as a `fn(&str) -> Option<String>`: `Some` non-empty wins,
//!   `None` (the Python `except Exception` branch) and `Some("")` (the
//!   `or token` branch) both fall back to the raw credential value. The
//!   default adapter passes plaintext through; the accounts layer
//!   (PIDASHCONV-145) supplies the real decryptor.
//! * Datetimes cross as pre-rendered ISO-8601 strings (`Option<String>`,
//!   the `dtos.rs` convention). [`parse_dt`] mirrors
//!   `datetime.fromisoformat(value.replace("Z", "+00:00"))` over the
//!   extended-format subset GitLab emits
//!   (`YYYY-MM-DD[T|space HH:MM[:SS[.ffffff]]][±HH:MM]`); anything else is
//!   `None`, mirroring the `except ValueError` branch. Rendering is the
//!   Python `datetime.isoformat()` form (fraction scaled to microseconds,
//!   omitted when zero; offset echoed).
//! * URL splitting is a minimal `scheme://authority/path` splitter matching
//!   `urllib.parse.urlparse` for the `http`/`https`/`ssh` shapes this
//!   adapter handles (scheme lowercased, like `urlparse`).
//!
//! Ported bugs (translate, don't redesign — each asserted in the tests
//! below and listed in the PR):
//!
//! * `_normalize_host` checks the scheme prefix case-sensitively, so an
//!   uppercase scheme gets a second `https://` prepended and normalizes to
//!   `https://https:` (`gitlab.py:37-46`, fixture `bug_uppercase_scheme`).
//! * `normalize_webhook` tests `"merge_request" in event.lower()`, which
//!   never matches a real `Merge Request Hook` header (spaces, not
//!   underscores), so `code_review_ref` is `{}` for genuine MR webhooks
//!   (`gitlab.py:400-412`).
//! * `list_issue_comments` hardcodes `web_url=""`, and `post_issue_comment`
//!   never sets `web_url` (it defaults to `""`) (`gitlab.py:341-380`).
//! * `parse_code_review_url` keeps the whole `netloc` (no `userinfo@`
//!   strip), unlike `parse_repo_url` which strips it (`gitlab.py:239-261`).

use std::collections::BTreeSet;

use pidash_types::integrations::{
    GitProviderAdapter, GitProviderCapabilities, GitProviderError, ParsedCodeReview,
    ParsedRepository, ProviderWebhookEvent, RemoteCodeReview, RemoteComment, RemoteIssue,
    RemoteRepository, RepositoryPage,
};
use serde_json::Value;

/// `DEFAULT_TIMEOUT_SECONDS` (`gitlab.py:34`).
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 30;

/// `User-Agent` sent on every API request (`gitlab.py:99`).
pub const USER_AGENT: &str = "pi-dash-gitlab-sync";

/// `_normalize_host` (`gitlab.py:37-46`).
///
/// Trims whitespace and trailing slashes, defaults empty input to
/// `https://gitlab.com`, prepends `https://` when the (case-sensitive)
/// `http://` / `https://` prefix is absent, then rebuilds
/// `{lower scheme}://{lower authority}` for `http(s)` URLs with a
/// non-empty authority. Anything else is returned slash-trimmed.
///
/// The case-sensitive prefix check is a ported bug: `"HTTPS://H/"`
/// normalizes to `"https://https:"` (see module docs).
pub fn normalize_host(host: &str) -> String {
    let trimmed = host.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return "https://gitlab.com".to_string();
    }
    let with_scheme = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    if let Some((scheme, rest)) = with_scheme.split_once("://") {
        let scheme = scheme.to_lowercase();
        let authority = rest.split('/').next().unwrap_or("");
        if (scheme == "http" || scheme == "https") && !authority.is_empty() {
            return format!("{}://{}", scheme, authority.to_lowercase());
        }
    }
    with_scheme.trim_end_matches('/').to_string()
}

/// Instance-level GitLab settings (`gitlab.py:49-62`).
///
/// `gitlab_host` is `settings.GITLAB_HOST` (`""` = unset);
/// `allowed_hosts` is `settings.GITLAB_ALLOWED_HOSTS`, where each entry
/// may itself hold a comma-joined string (the setting accepts a `str` or a
/// list, `gitlab.py:56-57`).
#[derive(Debug, Clone, Default)]
pub struct GitLabConfig {
    pub gitlab_host: String,
    pub allowed_hosts: Vec<String>,
}

impl GitLabConfig {
    /// `_allowed_hosts` (`gitlab.py:49-62`): `https://gitlab.com`, plus the
    /// normalized `GITLAB_HOST` when set, plus every normalized entry of
    /// `GITLAB_ALLOWED_HOSTS` (comma-joined entries split first).
    pub fn allowed_host_set(&self) -> BTreeSet<String> {
        let mut configured = BTreeSet::from(["https://gitlab.com".to_string()]);
        if !self.gitlab_host.is_empty() {
            configured.insert(normalize_host(&self.gitlab_host));
        }
        for entry in &self.allowed_hosts {
            for host in entry.split(',') {
                if host.is_empty() {
                    continue;
                }
                configured.insert(normalize_host(host));
            }
        }
        configured
    }
}

/// Days per month for `fromisoformat` parity (leap years included:
/// February 1900 has 28 days, February 2000 has 29).
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Parses exactly two ASCII digits.
fn two_digits(s: &str) -> Option<i64> {
    if s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit()) {
        s.parse().ok()
    } else {
        None
    }
}

/// Parsed `datetime.fromisoformat` components for [`parse_dt`].
struct ParsedDateTime {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    microsecond: i64,
    offset: Option<(char, i64, i64)>,
}

impl ParsedDateTime {
    /// Renders the `datetime.isoformat()` form: fraction scaled to
    /// microseconds and omitted when zero, offset echoed (naive datetimes
    /// carry no suffix).
    fn render(&self) -> String {
        let mut out = format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        );
        if self.microsecond != 0 {
            out.push_str(&format!(".{:06}", self.microsecond));
        }
        if let Some((sign, oh, om)) = self.offset {
            out.push(sign);
            out.push_str(&format!("{oh:02}:{om:02}"));
        }
        out
    }
}

/// Extended-format `datetime.fromisoformat` subset (see module docs).
/// Returns the `isoformat()` rendering, or `None` for anything Python
/// would raise `ValueError` on.
fn parse_iso8601(value: &str) -> Option<String> {
    let b = value.as_bytes();
    if b.len() < 10 {
        return None;
    }
    if b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let (year, month, day) = (
        value[0..4].parse::<i64>().ok()?,
        two_digits(&value[5..7])?,
        two_digits(&value[8..10])?,
    );
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) {
        return None;
    }
    if day < 1 || day > days_in_month(year, month) {
        return None;
    }
    let mut rest = &value[10..];
    let (hour, minute, second, microsecond) = if rest.is_empty() {
        (0, 0, 0, 0)
    } else {
        let sep = rest.as_bytes()[0];
        if sep.is_ascii_digit() || rest.len() < 5 {
            return None;
        }
        rest = &rest[1..];
        let hour = two_digits(rest.get(0..2)?)?;
        if rest.get(2..3)? != ":" {
            return None;
        }
        let minute = two_digits(rest.get(3..5)?)?;
        if hour > 23 || minute > 59 {
            return None;
        }
        rest = rest.get(5..)?;
        if rest.starts_with(':') {
            let second = two_digits(rest.get(1..3)?)?;
            if second > 59 {
                return None;
            }
            rest = rest.get(3..)?;
            let microsecond = if rest.starts_with('.') {
                let end = rest[1..]
                    .find(|c: char| !c.is_ascii_digit())
                    .map(|i| i + 1)
                    .unwrap_or(rest.len());
                let frac = &rest[1..end];
                if frac.is_empty() || frac.len() > 6 {
                    return None;
                }
                let mut scaled = frac.to_string();
                while scaled.len() < 6 {
                    scaled.push('0');
                }
                rest = &rest[end..];
                scaled.parse::<i64>().ok()?
            } else {
                0
            };
            (hour, minute, second, microsecond)
        } else {
            (hour, minute, 0, 0)
        }
    };
    let offset = if rest.is_empty() {
        None
    } else {
        let sign = rest.as_bytes()[0];
        if sign != b'+' && sign != b'-' {
            return None;
        }
        let digits: String = rest[1..].chars().filter(|c| c.is_ascii_digit()).collect();
        let (oh, om) = match digits.len() {
            2 => (digits.parse::<i64>().ok()?, 0),
            4 => (
                digits[0..2].parse::<i64>().ok()?,
                digits[2..4].parse::<i64>().ok()?,
            ),
            _ => return None,
        };
        // A colon, when present, must sit exactly between the pairs
        // (`+HH:MM`); anything else trailing is rejected below.
        let tail = &rest[1..];
        if tail.contains(':') && tail != format!("{oh:02}:{om:02}") {
            return None;
        }
        if oh > 23 || om > 59 {
            return None;
        }
        if tail.len() != 2 && tail.len() != 4 && tail.len() != 5 {
            return None;
        }
        Some((sign as char, oh, om))
    };
    Some(
        ParsedDateTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
            microsecond,
            offset,
        }
        .render(),
    )
}

/// `_parse_dt` (`gitlab.py:65-73`).
///
/// Empty input is `None`; every `Z` becomes `+00:00` (verbatim: the
/// replacement is not suffix-anchored); unparseable input is `None`.
/// The `Some` payload is the `datetime.isoformat()` rendering the DTOs
/// carry.
pub fn parse_dt(value: Option<&str>) -> Option<String> {
    let raw = value.filter(|v| !v.is_empty())?;
    parse_iso8601(&raw.replace('Z', "+00:00"))
}

/// `_strip_git_suffix` (`gitlab.py:74-77`).
pub fn strip_git_suffix(value: &str) -> &str {
    value.strip_suffix(".git").unwrap_or(value)
}

/// `_split_full_path` (`gitlab.py:78-83`): strips surrounding slashes,
/// then one `.git` suffix; subgroup paths split at the LAST slash into
/// `(namespace, name)`. Slash-less input is `None`.
pub fn split_full_path(path: &str) -> Option<(String, String)> {
    let trimmed = strip_git_suffix(path.trim_matches('/'));
    if trimmed.is_empty() || !trimmed.contains('/') {
        return None;
    }
    let (namespace, name) = trimmed.rsplit_once('/')?;
    Some((namespace.to_string(), name.to_string()))
}

/// `urllib.parse.quote(value, safe="")`: UTF-8 bytes percent-encoded with
/// uppercase hex; `[0-9A-Za-z_.~-]` never quoted (`gitlab.py:155` and
/// friends quote project paths, so `/` becomes `%2F`).
pub fn url_quote(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Minimal `urlparse` split for the adapter's URL shapes: `(lowercased
/// scheme, authority, path)` at the first `://`; no `://` means an empty
/// scheme and authority with the whole input as path.
fn split_url(candidate: &str) -> (String, String, String) {
    match candidate.split_once("://") {
        Some((scheme, rest)) => match rest.find('/') {
            Some(i) => (
                scheme.to_lowercase(),
                rest[..i].to_string(),
                rest[i..].to_string(),
            ),
            None => (scheme.to_lowercase(), rest.to_string(), String::new()),
        },
        None => (String::new(), String::new(), candidate.to_string()),
    }
}

/// Lowercased scheme of a normalized host URL (`_client`'s HTTPS gate,
/// `gitlab.py:203`).
fn url_scheme(url: &str) -> String {
    url.split_once("://")
        .map(|(scheme, _)| scheme.to_lowercase())
        .unwrap_or_default()
}

/// `value or ""` for `str(...)` fields: `None`/missing is `""`, `false`
/// / `0` / `""` / empty containers are `""` (falsy `or ""`), `true`
/// renders with the Python spelling `"True"`, other numbers render
/// plainly, strings pass through. Non-scalar truthy values render as
/// compact JSON (unreachable for real provider payloads).
fn str_or_empty(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => String::new(),
        Value::Number(n) => {
            if n.as_i64() == Some(0) || n.as_u64() == Some(0) || n.as_f64() == Some(0.0) {
                String::new()
            } else {
                n.to_string()
            }
        }
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `payload.get(key) or ""`.
fn field_str(payload: &Value, key: &str) -> String {
    payload.get(key).map(str_or_empty).unwrap_or_default()
}

/// Python truthiness for JSON values (`bool(...)`, `or`, `if` guards).
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            n.as_i64() != Some(0) && n.as_u64() != Some(0) && n.as_f64() != Some(0.0)
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `(payload.get(outer) or {}).get(inner) or ""`.
fn nested_str(payload: &Value, outer: &str, inner: &str) -> String {
    match payload.get(outer) {
        Some(Value::Object(map)) => map.get(inner).map(str_or_empty).unwrap_or_default(),
        _ => String::new(),
    }
}

/// First non-empty string header (`headers.get(a) or headers.get(b)`).
fn header_str<'a>(headers: &'a Value, key: &str) -> &'a str {
    headers
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("")
}

/// `payload.get(key) or {}` as an owned value.
fn ref_or_empty_object(payload: &Value, key: &str) -> Value {
    match payload.get(key) {
        Some(v) if is_truthy(v) => v.clone(),
        _ => Value::Object(Default::default()),
    }
}

/// One API round trip as the client sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLabRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// GET query pairs, in call order (`requests` `params=`).
    pub query: Vec<(String, String)>,
    /// POST form pairs (`requests` `data=`).
    pub form: Vec<(String, String)>,
    pub timeout_secs: u64,
}

/// One API round trip as the client sees it.
///
/// The transport MUST surface redirect statuses instead of following them
/// (`allow_redirects=False`, `gitlab.py:104`); the client maps 3xx itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLabResponse {
    pub status: u16,
    /// Response headers; looked up case-insensitively (`requests`
    /// `CaseInsensitiveDict`, `gitlab.py:122,131`).
    pub headers: Vec<(String, String)>,
    /// Decoded response text (`response.text` / `response.json()` source).
    pub body: String,
}

impl GitLabResponse {
    /// Case-insensitive header lookup.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// `response.json()`; undecodable bodies are `General` (in Python the
    /// decoder error propagates and the view renders it 400 — same
    /// observable status).
    pub fn json(&self) -> Result<Value, GitProviderError> {
        serde_json::from_str(&self.body).map_err(|err| GitProviderError::General(err.to_string()))
    }
}

/// The `requests.request` seam (`GitLabClient._request` calls exactly one
/// method per round trip, `gitlab.py:102-118`).
pub trait GitLabTransport {
    fn request(&self, request: &GitLabRequest) -> Result<GitLabResponse, GitProviderError>;
}

/// `GitLabClient` (`gitlab.py:86-181`).
pub struct GitLabClient<'a, T: GitLabTransport> {
    token: String,
    host_url: String,
    api_base: String,
    timeout_secs: u64,
    transport: &'a T,
}

impl<'a, T: GitLabTransport> GitLabClient<'a, T> {
    /// `__init__` (`gitlab.py:87-93`): empty tokens are rejected here, as
    /// in Python (`GitProviderAuthError("GitLab token is missing")`).
    pub fn new(token: String, host_url: &str, transport: &'a T) -> Result<Self, GitProviderError> {
        if token.is_empty() {
            return Err(GitProviderError::Auth("GitLab token is missing".into()));
        }
        let host_url = normalize_host(host_url);
        let api_base = format!("{host_url}/api/v4");
        Ok(Self {
            token,
            host_url,
            api_base,
            timeout_secs: DEFAULT_TIMEOUT_SECONDS,
            transport,
        })
    }

    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    pub fn host_url(&self) -> &str {
        &self.host_url
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    /// `_headers` (`gitlab.py:95-100`).
    pub fn headers(&self) -> Vec<(String, String)> {
        vec![
            ("PRIVATE-TOKEN".to_string(), self.token.clone()),
            ("Accept".to_string(), "application/json".to_string()),
            ("User-Agent".to_string(), USER_AGENT.to_string()),
        ]
    }

    /// `_request` (`gitlab.py:102-118`).
    ///
    /// Absolute `http...` paths go out verbatim after the configured-host
    /// guard; anything else is appended to `<host>/api/v4`. Status mapping
    /// is verbatim (3xx redirect guard, 401/403/404); any other 4xx/5xx
    /// becomes `General`, which the views render 400 — the same observable
    /// status as the propagating `requests.HTTPError` in Python.
    pub fn request(
        &self,
        method: &str,
        path: &str,
        query: Vec<(String, String)>,
        form: Vec<(String, String)>,
    ) -> Result<GitLabResponse, GitProviderError> {
        let url = if path.starts_with("http") {
            if normalize_host(path) != self.host_url {
                return Err(GitProviderError::Permission(
                    "GitLab API request target does not match the configured host".into(),
                ));
            }
            path.to_string()
        } else {
            format!("{}{}", self.api_base, path)
        };
        let response = self.transport.request(&GitLabRequest {
            method: method.to_string(),
            url,
            headers: self.headers(),
            query,
            form,
            timeout_secs: self.timeout_secs,
        })?;
        let status = response.status;
        if (300..400).contains(&status) {
            return Err(GitProviderError::Permission(
                "GitLab API redirects are not followed".into(),
            ));
        }
        if status == 401 {
            return Err(GitProviderError::Auth(response.body.clone()));
        }
        if status == 403 {
            return Err(GitProviderError::Permission(response.body.clone()));
        }
        if status == 404 {
            return Err(GitProviderError::NotFound(response.body.clone()));
        }
        if status >= 400 {
            return Err(GitProviderError::General(response.body.clone()));
        }
        Ok(response)
    }

    /// `_has_next` (`gitlab.py:120-122`): a non-empty `X-Next-Page`
    /// header (case-insensitive, mirroring `requests`).
    pub fn has_next(response: &GitLabResponse) -> bool {
        response
            .header("X-Next-Page")
            .is_some_and(|v| !v.is_empty())
    }

    /// `_paginate` (`gitlab.py:124-134`): `per_page` defaults to 100 and
    /// `page` to 1; each `X-Next-Page` value becomes the next `page`.
    pub fn paginate(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<Vec<Value>, GitProviderError> {
        let mut query: Vec<(String, String)> = params
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        upsert_param(&mut query, "per_page", "100");
        upsert_param(&mut query, "page", "1");
        let mut items = Vec::new();
        loop {
            let response = self.request("GET", path, query.clone(), Vec::new())?;
            match response.json()? {
                Value::Array(page_items) => items.extend(page_items),
                _ => {
                    return Err(GitProviderError::General(
                        "GitLab API returned a non-list page".into(),
                    ));
                }
            }
            match response.header("X-Next-Page") {
                Some(next) if !next.is_empty() => {
                    upsert_param(&mut query, "page", next);
                }
                _ => break,
            }
        }
        Ok(items)
    }

    /// `get_authenticated_user` (`gitlab.py:136-137`).
    pub fn get_authenticated_user(&self) -> Result<Value, GitProviderError> {
        self.request("GET", "/user", Vec::new(), Vec::new())?.json()
    }

    /// `list_projects` (`gitlab.py:139-152`).
    pub fn list_projects(
        &self,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<Value>, bool), GitProviderError> {
        let query = vec![
            ("membership".to_string(), "True".to_string()),
            ("simple".to_string(), "True".to_string()),
            ("order_by".to_string(), "last_activity_at".to_string()),
            ("sort".to_string(), "desc".to_string()),
            ("page".to_string(), page.to_string()),
            ("per_page".to_string(), per_page.to_string()),
        ];
        let response = self.request("GET", "/projects", query, Vec::new())?;
        let has_next = Self::has_next(&response);
        match response.json()? {
            Value::Array(projects) => Ok((projects, has_next)),
            _ => Err(GitProviderError::General(
                "GitLab API returned a non-list project page".into(),
            )),
        }
    }

    /// `get_project` (`gitlab.py:154-156`).
    pub fn get_project(&self, full_path_or_id: &str) -> Result<Value, GitProviderError> {
        self.request(
            "GET",
            &format!("/projects/{}", url_quote(full_path_or_id)),
            Vec::new(),
            Vec::new(),
        )?
        .json()
    }

    /// `list_open_issues` (`gitlab.py:158-162`).
    pub fn list_open_issues(&self, project_id: &str) -> Result<Vec<Value>, GitProviderError> {
        self.paginate(
            &format!("/projects/{}/issues", url_quote(project_id)),
            &[
                ("state", "opened"),
                ("order_by", "updated_at"),
                ("sort", "desc"),
            ],
        )
    }

    /// `list_issue_notes` (`gitlab.py:164-168`).
    pub fn list_issue_notes(
        &self,
        project_id: &str,
        issue_iid: &str,
    ) -> Result<Vec<Value>, GitProviderError> {
        self.paginate(
            &format!(
                "/projects/{}/issues/{issue_iid}/notes",
                url_quote(project_id)
            ),
            &[("order_by", "updated_at"), ("sort", "asc")],
        )
    }

    /// `post_issue_note` (`gitlab.py:170-175`).
    pub fn post_issue_note(
        &self,
        project_id: &str,
        issue_iid: &str,
        body: &str,
    ) -> Result<Value, GitProviderError> {
        self.request(
            "POST",
            &format!(
                "/projects/{}/issues/{issue_iid}/notes",
                url_quote(project_id)
            ),
            Vec::new(),
            vec![("body".to_string(), body.to_string())],
        )?
        .json()
    }

    /// `get_merge_request` (`gitlab.py:177-181`).
    pub fn get_merge_request(
        &self,
        project_id: &str,
        mr_iid: &str,
    ) -> Result<Value, GitProviderError> {
        self.request(
            "GET",
            &format!(
                "/projects/{}/merge_requests/{mr_iid}",
                url_quote(project_id)
            ),
            Vec::new(),
            Vec::new(),
        )?
        .json()
    }
}

/// `dict.setdefault` for ordered query pairs: fills the key only when
/// absent, replaces it when present (the `_paginate` next-page update,
/// `gitlab.py:127,134`).
fn upsert_param(params: &mut Vec<(String, String)>, key: &str, value: &str) {
    match params.iter_mut().find(|(k, _)| k == key) {
        Some((_, v)) => *v = value.to_string(),
        None => params.push((key.to_string(), value.to_string())),
    }
}

/// Matches the scp-like `git@host:path` form (`GitLabAdapter._ssh_repo_re`,
/// `gitlab.py:189`).
///
/// The regex `^git@(?P<host>[^:]+):(?P<path>.+?)(?:\.git)?$` is
/// case-sensitive, never spans newlines (`.`), and prefers stripping one
/// trailing `.git` whenever the remainder stays non-empty — i.e. it strips
/// exactly when the path part is longer than `.git` itself. This port
/// reproduces all three properties without a regex dependency.
fn match_ssh_repo(candidate: &str) -> Option<(String, String)> {
    let rest = candidate.strip_prefix("git@")?;
    if rest.contains('\n') {
        return None;
    }
    let (host, mut path) = rest.split_once(':')?;
    if host.is_empty() || path.is_empty() {
        return None;
    }
    if path.len() > 4 && path.ends_with(".git") {
        path = &path[..path.len() - 4];
    }
    Some((host.to_string(), path.to_string()))
}

/// Adapter registration key (`gitlab.py:185`).
pub const ADAPTER_KEY: &str = "gitlab";

/// `GitLabAdapter` (`gitlab.py:184-412`, key `"gitlab"`).
///
/// `decrypt` is `decrypt_data` (`gitlab.py:195`); see the module docs for
/// the `None`-means-`except` contract. [`GitLabAdapter::new`] passes
/// tokens through untouched for callers whose credentials already hold
/// plaintext.
pub struct GitLabAdapter<'a, T: GitLabTransport> {
    config: GitLabConfig,
    transport: &'a T,
    decrypt: fn(&str) -> Option<String>,
}

/// Plaintext passthrough: `None` falls back to the raw credential value,
/// exactly like an undecryptable token in `_token`.
fn passthrough_token(_: &str) -> Option<String> {
    None
}

impl<'a, T: GitLabTransport> GitLabAdapter<'a, T> {
    pub fn new(transport: &'a T) -> Self {
        Self {
            config: GitLabConfig::default(),
            transport,
            decrypt: passthrough_token,
        }
    }

    pub fn with_config(config: GitLabConfig, transport: &'a T) -> Self {
        Self {
            config,
            transport,
            decrypt: passthrough_token,
        }
    }

    pub fn with_decrypt(
        config: GitLabConfig,
        transport: &'a T,
        decrypt: fn(&str) -> Option<String>,
    ) -> Self {
        Self {
            config,
            transport,
            decrypt,
        }
    }

    /// `_token` (`gitlab.py:191-198`): missing token is `Auth`; otherwise
    /// `decrypt_data(token) or token`, with a failed decrypt falling back
    /// to the raw value.
    pub fn resolve_token(credential: &Value) -> Result<String, GitProviderError> {
        Self::resolve_token_with(credential, &passthrough_token)
    }

    fn resolve_token_with(
        credential: &Value,
        decrypt: &dyn Fn(&str) -> Option<String>,
    ) -> Result<String, GitProviderError> {
        let token = credential
            .get("token")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if token.is_empty() {
            return Err(GitProviderError::Auth("GitLab token is missing".into()));
        }
        match decrypt(token) {
            Some(decrypted) if !decrypted.is_empty() => Ok(decrypted),
            _ => Ok(token.to_string()),
        }
    }

    /// `_client` (`gitlab.py:200-209`): credential `host_url`, else the
    /// configured `GITLAB_HOST`, else `https://gitlab.com`; HTTPS is
    /// enforced before the allowlist gate.
    fn client(&self, credential: &Value) -> Result<GitLabClient<'_, T>, GitProviderError> {
        let configured =
            (!self.config.gitlab_host.is_empty()).then_some(self.config.gitlab_host.as_str());
        let raw = credential
            .get("host_url")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or(configured)
            .unwrap_or("https://gitlab.com");
        let host_url = normalize_host(raw);
        if url_scheme(&host_url) != "https" {
            return Err(GitProviderError::Permission(
                "GitLab host must use HTTPS".into(),
            ));
        }
        if !self.host_allowed(&host_url) {
            return Err(GitProviderError::Permission(
                "GitLab host is not allowed; ask an instance admin to add it to GITLAB_ALLOWED_HOSTS."
                    .into(),
            ));
        }
        GitLabClient::new(
            Self::resolve_token_with(credential, &self.decrypt)?,
            &host_url,
            self.transport,
        )
    }

    /// `_host_allowed` (`gitlab.py:211-213`).
    pub fn host_allowed(&self, host_url: &str) -> bool {
        self.config
            .allowed_host_set()
            .contains(&normalize_host(host_url))
    }

    /// `parse_repo_url` (`gitlab.py:214-256`): scp-like `git@host:path`
    /// plus `http`/`https`/`ssh` URLs (the `user@` userinfo is stripped
    /// for the allowlist lookup, `gitlab.py:239`); `/-/...` view suffixes
    /// are cut; non-allowlisted hosts are `None`.
    pub fn parse_repo_url(&self, url: &str) -> Option<ParsedRepository> {
        let candidate = url.trim();
        if candidate.is_empty() {
            return None;
        }
        if let Some((host, path)) = match_ssh_repo(candidate) {
            let host_url = normalize_host(&host);
            if !self.host_allowed(&host_url) {
                return None;
            }
            let (namespace, name) = split_full_path(&path)?;
            return Some(ParsedRepository {
                provider: ADAPTER_KEY.to_string(),
                host_url,
                namespace: namespace.clone(),
                name: name.clone(),
                full_name: format!("{namespace}/{name}"),
                clone_url: candidate.to_string(),
            });
        }
        let (scheme, authority, path) = split_url(candidate);
        // `rsplit("@", 1)[-1]`: the part after the LAST `@`.
        let host_url = normalize_host(authority.rsplit('@').next().unwrap_or(""));
        if !matches!(scheme.as_str(), "http" | "https" | "ssh") || !self.host_allowed(&host_url) {
            return None;
        }
        let repo_path = path
            .split_once("/-/")
            .map(|(head, _)| head)
            .unwrap_or(&path);
        let (namespace, name) = split_full_path(repo_path)?;
        Some(ParsedRepository {
            provider: ADAPTER_KEY.to_string(),
            host_url,
            namespace: namespace.clone(),
            name: name.clone(),
            full_name: format!("{namespace}/{name}"),
            clone_url: candidate.to_string(),
        })
    }

    /// `parse_code_review_url` (`gitlab.py:258-282`): `http`/`https` only
    /// on an allowlisted host, `/-/merge_requests/<digits>`; the URL is
    /// rebuilt canonical. Note the ported asymmetry: the full `netloc`
    /// (userinfo included) feeds the allowlist lookup here.
    pub fn parse_code_review_url(&self, url: &str) -> Option<ParsedCodeReview> {
        let candidate = url.trim();
        if candidate.is_empty() {
            return None;
        }
        let (scheme, authority, path) = split_url(candidate);
        let host_url = normalize_host(&authority);
        if !matches!(scheme.as_str(), "http" | "https") || !self.host_allowed(&host_url) {
            return None;
        }
        let marker = "/-/merge_requests/";
        let at = path.find(marker)?;
        let mr_iid = path[at + marker.len()..]
            .trim_matches('/')
            .split('/')
            .next()
            .unwrap_or("");
        if mr_iid.is_empty() || !mr_iid.chars().all(|c| c.is_numeric()) {
            return None;
        }
        let (namespace, name) = split_full_path(&path[..at])?;
        Some(ParsedCodeReview {
            provider: ADAPTER_KEY.to_string(),
            host_url: host_url.clone(),
            namespace: namespace.clone(),
            repo_name: name.clone(),
            external_iid: mr_iid.to_string(),
            url: format!("{host_url}/{namespace}/{name}/-/merge_requests/{mr_iid}"),
        })
    }

    /// `verify_provider_account` (`gitlab.py:284-285`).
    pub fn verify_provider_account(&self, credential: &Value) -> Result<Value, GitProviderError> {
        self.client(credential)?.get_authenticated_user()
    }

    /// `credential_capabilities` (`gitlab.py:287-294`): constant — the
    /// credential is ignored.
    pub fn credential_capabilities(
        &self,
        _credential: &Value,
    ) -> Result<GitProviderCapabilities, GitProviderError> {
        Ok(GitProviderCapabilities {
            read_repositories: true,
            read_issues: true,
            write_comments: true,
            manage_webhooks: true,
            clone: false,
        })
    }

    /// `_remote_repo` (`gitlab.py:296-311`): `is_private` is exactly
    /// `visibility == "private"`; namespace/name come from
    /// `path_with_namespace` (else `path`), falling back to
    /// `("", path)` when the combined name has no slash.
    pub fn remote_repo(payload: &Value) -> RemoteRepository {
        let full_name = {
            let namespaced = field_str(payload, "path_with_namespace");
            if !namespaced.is_empty() {
                namespaced
            } else {
                field_str(payload, "path")
            }
        };
        let (namespace, name) =
            split_full_path(&full_name).unwrap_or((String::new(), field_str(payload, "path")));
        RemoteRepository {
            provider: ADAPTER_KEY.to_string(),
            external_id: field_str(payload, "id"),
            namespace,
            name,
            full_name,
            web_url: field_str(payload, "web_url"),
            clone_url_http: field_str(payload, "http_url_to_repo"),
            clone_url_ssh: field_str(payload, "ssh_url_to_repo"),
            default_branch: field_str(payload, "default_branch"),
            is_private: payload.get("visibility").and_then(|v| v.as_str()) == Some("private"),
            metadata: payload.clone(),
        }
    }

    /// `list_repositories` (`gitlab.py:313-319`).
    pub fn list_repositories(
        &self,
        credential: &Value,
        page: i64,
    ) -> Result<RepositoryPage, GitProviderError> {
        let (repos, has_next) = self.client(credential)?.list_projects(page, 100)?;
        Ok(RepositoryPage {
            repositories: repos.iter().map(Self::remote_repo).collect(),
            page,
            has_next_page: has_next,
        })
    }

    /// `get_repository` (`gitlab.py:321-323`).
    pub fn get_repository(
        &self,
        credential: &Value,
        parsed: &ParsedRepository,
    ) -> Result<RemoteRepository, GitProviderError> {
        Ok(Self::remote_repo(
            &self.client(credential)?.get_project(&parsed.full_name)?,
        ))
    }

    /// `list_open_issues` (`gitlab.py:324-339`): `external_id`, else the
    /// full name, selects the project.
    pub fn list_open_issues(
        &self,
        credential: &Value,
        repository: &RemoteRepository,
    ) -> Result<Vec<RemoteIssue>, GitProviderError> {
        let project_id = if !repository.external_id.is_empty() {
            repository.external_id.clone()
        } else {
            repository.full_name.clone()
        };
        Ok(self
            .client(credential)?
            .list_open_issues(&project_id)?
            .iter()
            .map(|issue| RemoteIssue {
                external_id: field_str(issue, "id"),
                external_iid: field_str(issue, "iid"),
                title: field_str(issue, "title"),
                body: field_str(issue, "description"),
                state: field_str(issue, "state"),
                author: nested_str(issue, "author", "username"),
                web_url: field_str(issue, "web_url"),
                created_at: parse_dt(issue.get("created_at").and_then(|v| v.as_str())),
                updated_at: parse_dt(issue.get("updated_at").and_then(|v| v.as_str())),
                metadata: issue.clone(),
            })
            .collect())
    }

    /// `list_issue_comments` (`gitlab.py:341-360`): system notes are
    /// skipped and `web_url` is always `""` (ported verbatim).
    pub fn list_issue_comments(
        &self,
        credential: &Value,
        repository: &RemoteRepository,
        issue_iid: &str,
    ) -> Result<Vec<RemoteComment>, GitProviderError> {
        let project_id = if !repository.external_id.is_empty() {
            repository.external_id.clone()
        } else {
            repository.full_name.clone()
        };
        Ok(self
            .client(credential)?
            .list_issue_notes(&project_id, issue_iid)?
            .iter()
            .filter(|note| !note.get("system").is_some_and(is_truthy))
            .map(|note| RemoteComment {
                external_id: field_str(note, "id"),
                body: field_str(note, "body"),
                author: nested_str(note, "author", "username"),
                web_url: String::new(),
                created_at: parse_dt(note.get("created_at").and_then(|v| v.as_str())),
                updated_at: parse_dt(note.get("updated_at").and_then(|v| v.as_str())),
                metadata: note.clone(),
            })
            .collect())
    }

    /// `post_issue_comment` (`gitlab.py:362-379`): the returned comment
    /// carries no `web_url` (the DTO default `""`).
    pub fn post_issue_comment(
        &self,
        credential: &Value,
        repository: &RemoteRepository,
        issue_iid: &str,
        body: &str,
    ) -> Result<RemoteComment, GitProviderError> {
        let project_id = if !repository.external_id.is_empty() {
            repository.external_id.clone()
        } else {
            repository.full_name.clone()
        };
        let note = self
            .client(credential)?
            .post_issue_note(&project_id, issue_iid, body)?;
        Ok(RemoteComment {
            external_id: field_str(&note, "id"),
            body: field_str(&note, "body"),
            author: nested_str(&note, "author", "username"),
            web_url: String::new(),
            created_at: parse_dt(note.get("created_at").and_then(|v| v.as_str())),
            updated_at: parse_dt(note.get("updated_at").and_then(|v| v.as_str())),
            metadata: note,
        })
    }

    /// `get_code_review` (`gitlab.py:381-398`): only the raw state
    /// `"merged"` stays merged; `"closed"` stays closed; everything else
    /// (including `""`) is `"open"`. `draft` is the truthy `draft` flag
    /// or a strict-`true` `work_in_progress`.
    pub fn get_code_review(
        &self,
        credential: &Value,
        parsed: &ParsedCodeReview,
    ) -> Result<RemoteCodeReview, GitProviderError> {
        let project_key = format!("{}/{}", parsed.namespace, parsed.repo_name);
        let mr = self
            .client(credential)?
            .get_merge_request(&project_key, &parsed.external_iid)?;
        let raw_state = field_str(&mr, "state");
        let state = if raw_state.is_empty() {
            "opened".to_string()
        } else {
            raw_state
        };
        let merged = state == "merged";
        let normalized_state = if merged {
            "merged"
        } else if state == "closed" {
            "closed"
        } else {
            "open"
        };
        let draft = mr.get("draft").is_some_and(is_truthy)
            || mr.get("work_in_progress") == Some(&Value::Bool(true));
        let external_iid = {
            let iid = field_str(&mr, "iid");
            if iid.is_empty() {
                parsed.external_iid.clone()
            } else {
                iid
            }
        };
        let web_url = {
            let url = field_str(&mr, "web_url");
            if url.is_empty() {
                parsed.url.clone()
            } else {
                url
            }
        };
        Ok(RemoteCodeReview {
            external_id: field_str(&mr, "id"),
            external_iid,
            title: field_str(&mr, "title"),
            state: normalized_state.to_string(),
            merged,
            draft,
            web_url,
            updated_at: parse_dt(mr.get("updated_at").and_then(|v| v.as_str())),
            metadata: mr,
        })
    }

    /// `normalize_webhook` (`gitlab.py:400-412`): header lookup is the two
    /// exact `X-Gitlab-Event` spellings; empty bodies decode as `{}`;
    /// `code_review_ref`/`issue_ref` are the verbatim substring guards
    /// (see the ported-bug note in the module docs).
    pub fn normalize_webhook(
        raw_body: &[u8],
        headers: &Value,
    ) -> Result<ProviderWebhookEvent, GitProviderError> {
        let text = std::str::from_utf8(raw_body)
            .map_err(|err| GitProviderError::General(err.to_string()))?;
        let text = if text.is_empty() { "{}" } else { text };
        let payload: Value =
            serde_json::from_str(text).map_err(|err| GitProviderError::General(err.to_string()))?;
        let event = {
            let first = header_str(headers, "X-Gitlab-Event");
            if !first.is_empty() {
                first.to_string()
            } else {
                header_str(headers, "X-GitLab-Event").to_string()
            }
        };
        let attrs = match payload.get("object_attributes") {
            Some(v) if is_truthy(v) => v.clone(),
            _ => Value::Object(Default::default()),
        };
        let action = {
            let direct = attrs.get("action").and_then(|v| v.as_str()).unwrap_or("");
            if !direct.is_empty() {
                direct.to_string()
            } else {
                payload
                    .get("event_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            }
        };
        let lowered = event.to_lowercase();
        Ok(ProviderWebhookEvent {
            provider: ADAPTER_KEY.to_string(),
            event: event.clone(),
            action,
            repository_ref: ref_or_empty_object(&payload, "project"),
            code_review_ref: if lowered.contains("merge_request") {
                attrs.clone()
            } else {
                Value::Object(Default::default())
            },
            issue_ref: if lowered.contains("issue") {
                attrs
            } else {
                Value::Object(Default::default())
            },
            payload,
        })
    }
}

impl<T: GitLabTransport> GitProviderAdapter for GitLabAdapter<'_, T> {
    const KEY: &'static str = ADAPTER_KEY;
    const DISPLAY_NAME: &'static str = "GitLab";
    const CODE_REVIEW_TERM: &'static str = "merge request";

    fn parse_repo_url(&self, url: &str) -> Option<ParsedRepository> {
        GitLabAdapter::parse_repo_url(self, url)
    }

    fn parse_code_review_url(&self, url: &str) -> Option<ParsedCodeReview> {
        GitLabAdapter::parse_code_review_url(self, url)
    }

    fn verify_provider_account(&self, credential: &Value) -> Result<Value, GitProviderError> {
        GitLabAdapter::verify_provider_account(self, credential)
    }

    fn credential_capabilities(
        &self,
        credential: &Value,
    ) -> Result<GitProviderCapabilities, GitProviderError> {
        GitLabAdapter::credential_capabilities(self, credential)
    }

    fn list_repositories(
        &self,
        credential: &Value,
        page: i64,
    ) -> Result<RepositoryPage, GitProviderError> {
        GitLabAdapter::list_repositories(self, credential, page)
    }

    fn get_repository(
        &self,
        credential: &Value,
        parsed: &ParsedRepository,
    ) -> Result<RemoteRepository, GitProviderError> {
        GitLabAdapter::get_repository(self, credential, parsed)
    }

    fn list_open_issues(
        &self,
        credential: &Value,
        repository: &RemoteRepository,
    ) -> Result<Vec<RemoteIssue>, GitProviderError> {
        GitLabAdapter::list_open_issues(self, credential, repository)
    }

    fn list_issue_comments(
        &self,
        credential: &Value,
        repository: &RemoteRepository,
        issue_iid: &str,
    ) -> Result<Vec<RemoteComment>, GitProviderError> {
        GitLabAdapter::list_issue_comments(self, credential, repository, issue_iid)
    }

    fn post_issue_comment(
        &self,
        credential: &Value,
        repository: &RemoteRepository,
        issue_iid: &str,
        body: &str,
    ) -> Result<RemoteComment, GitProviderError> {
        GitLabAdapter::post_issue_comment(self, credential, repository, issue_iid, body)
    }

    fn get_code_review(
        &self,
        credential: &Value,
        parsed: &ParsedCodeReview,
    ) -> Result<RemoteCodeReview, GitProviderError> {
        GitLabAdapter::get_code_review(self, credential, parsed)
    }

    fn normalize_webhook(
        &self,
        raw_body: &[u8],
        headers: &Value,
    ) -> Result<ProviderWebhookEvent, GitProviderError> {
        GitLabAdapter::<'_, T>::normalize_webhook(raw_body, headers)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    /// Recording fake: asserts the client's exact wire contract
    /// (method, URL, headers, query/form pairs) and replays canned
    /// responses in order.
    struct FakeTransport {
        calls: RefCell<Vec<GitLabRequest>>,
        responses: RefCell<VecDeque<GitLabResponse>>,
    }

    impl FakeTransport {
        fn new(responses: Vec<GitLabResponse>) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                responses: RefCell::new(responses.into()),
            }
        }

        fn calls(&self) -> Vec<GitLabRequest> {
            self.calls.borrow().clone()
        }
    }

    impl GitLabTransport for FakeTransport {
        fn request(&self, request: &GitLabRequest) -> Result<GitLabResponse, GitProviderError> {
            self.calls.borrow_mut().push(request.clone());
            self.responses.borrow_mut().pop_front().ok_or_else(|| {
                GitProviderError::General("fake transport ran out of responses".into())
            })
        }
    }

    fn ok(body: Value) -> GitLabResponse {
        GitLabResponse {
            status: 200,
            headers: Vec::new(),
            body: body.to_string(),
        }
    }

    fn status_response(status: u16, body: &str) -> GitLabResponse {
        GitLabResponse {
            status,
            headers: Vec::new(),
            body: body.to_string(),
        }
    }

    fn gitlab_golden() -> Value {
        let path = format!(
            "{}/../../fixtures/integrations/adapters/gitlab.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    #[test]
    fn golden_file_loads() {
        let golden = gitlab_golden();
        assert!(golden.get("_normalize_host").is_some());
        assert!(golden.get("parse_repo_url").is_some());
    }

    #[test]
    fn normalize_host_replays_golden() {
        let golden = gitlab_golden();
        let cases = &golden["_normalize_host"];
        assert_eq!(
            normalize_host("gitlab.example.com"),
            cases["bare_hostname"].as_str().unwrap()
        );
        assert_eq!(
            normalize_host(""),
            cases["empty_defaults_to_gitlab_com"].as_str().unwrap()
        );
        // `http://` survives (the HTTPS gate rejects it later).
        assert!(cases["http_scheme_kept"]
            .as_str()
            .unwrap()
            .starts_with("http://gitlab.example.com"));
        assert_eq!(
            normalize_host("http://gitlab.example.com"),
            "http://gitlab.example.com"
        );
        assert_eq!(
            normalize_host("  https://GitLab.Example.COM/  "),
            "https://gitlab.example.com"
        );
    }

    #[test]
    fn normalize_host_uppercase_scheme_bug_ported() {
        // Fixture `bug_uppercase_scheme`: the prefix check is case-sensitive,
        // so `HTTPS://...` gains a second `https://` and urlparse yields
        // netloc `https:`.
        let golden = gitlab_golden();
        let bug = &golden["bug_uppercase_scheme"];
        assert_eq!(
            normalize_host(bug["input"].as_str().unwrap()),
            bug["observed"].as_str().unwrap()
        );
        assert_eq!(
            normalize_host("HTTPS://GitLab.Example.COM/"),
            "https://https:"
        );
    }

    #[test]
    fn allowed_hosts_default_and_config() {
        let plain = GitLabConfig::default();
        assert_eq!(
            plain.allowed_host_set(),
            BTreeSet::from(["https://gitlab.com".to_string()])
        );
        // `GITLAB_HOST` joins the set after normalization.
        let with_host = GitLabConfig {
            gitlab_host: "gitlab.example.com".to_string(),
            allowed_hosts: vec![],
        };
        assert!(with_host
            .allowed_host_set()
            .contains("https://gitlab.example.com"));
        assert!(with_host.allowed_host_set().contains("https://gitlab.com"));
        // List entries and comma-joined strings both land normalized.
        let mixed = GitLabConfig {
            gitlab_host: String::new(),
            allowed_hosts: vec![
                "https://a.example.com".to_string(),
                "b.example.com,https://c.example.com/".to_string(),
                String::new(),
            ],
        };
        let set = mixed.allowed_host_set();
        assert!(set.contains("https://a.example.com"));
        assert!(set.contains("https://b.example.com"));
        assert!(set.contains("https://c.example.com"));
    }

    #[test]
    fn parse_dt_shapes() {
        assert_eq!(
            parse_dt(Some("2024-01-02T03:04:05Z")),
            Some("2024-01-02T03:04:05+00:00".to_string())
        );
        assert_eq!(
            parse_dt(Some("2024-01-02T03:04:05+00:00")),
            Some("2024-01-02T03:04:05+00:00".to_string())
        );
        assert_eq!(
            parse_dt(Some("2024-01-02T03:04:05+02:00")),
            Some("2024-01-02T03:04:05+02:00".to_string())
        );
        // Zero fractions vanish (microsecond == 0 renders bare seconds);
        // nonzero fractions are scaled to microseconds.
        assert_eq!(
            parse_dt(Some("2024-01-02T03:04:05.000Z")),
            Some("2024-01-02T03:04:05+00:00".to_string())
        );
        assert_eq!(
            parse_dt(Some("2024-01-02T03:04:05.123Z")),
            Some("2024-01-02T03:04:05.123000+00:00".to_string())
        );
        assert_eq!(parse_dt(None), None);
        assert_eq!(parse_dt(Some("")), None);
        assert_eq!(parse_dt(Some("garbage")), None);
        assert_eq!(parse_dt(Some("2024-13-02T03:04:05Z")), None);
        assert_eq!(parse_dt(Some("2024-02-30T03:04:05Z")), None);
        assert_eq!(parse_dt(Some("2024-01-02T25:04:05Z")), None);
        // Date-only and naive inputs keep the `isoformat()` shape.
        assert_eq!(
            parse_dt(Some("2024-01-02")),
            Some("2024-01-02T00:00:00".to_string())
        );
        assert_eq!(
            parse_dt(Some("2024-01-02 03:04:05")),
            Some("2024-01-02T03:04:05".to_string())
        );
    }

    #[test]
    fn split_full_path_subgroups() {
        assert_eq!(
            split_full_path("platform/backend/api"),
            Some(("platform/backend".to_string(), "api".to_string()))
        );
        assert_eq!(
            split_full_path("/platform/backend/api.git"),
            Some(("platform/backend".to_string(), "api".to_string()))
        );
        assert_eq!(split_full_path("lonely"), None);
        assert_eq!(split_full_path(""), None);
        assert_eq!(split_full_path(".git"), None);
        assert_eq!(strip_git_suffix("api.git"), "api");
        assert_eq!(strip_git_suffix("api"), "api");
    }

    #[test]
    fn match_ssh_repo_shapes() {
        assert_eq!(
            match_ssh_repo("git@gitlab.example.com:platform/backend/api.git"),
            Some((
                "gitlab.example.com".to_string(),
                "platform/backend/api".to_string()
            ))
        );
        assert_eq!(
            match_ssh_repo("git@h:a.git"),
            Some(("h".to_string(), "a".to_string()))
        );
        // Non-greedy `.git` edge: a bare `.git` path is kept whole, so the
        // later split rejects it — exactly like the regex.
        assert_eq!(
            match_ssh_repo("git@h:.git"),
            Some(("h".to_string(), ".git".to_string()))
        );
        assert_eq!(match_ssh_repo("GIT@h:a/b"), None);
        assert_eq!(match_ssh_repo("git@:a/b"), None);
        assert_eq!(match_ssh_repo("git@h:"), None);
        assert_eq!(match_ssh_repo("https://h/a/b"), None);
    }

    #[test]
    fn url_quote_encodes_path() {
        assert_eq!(
            url_quote("platform/backend/api"),
            "platform%2Fbackend%2Fapi"
        );
        assert_eq!(url_quote("123"), "123");
        assert_eq!(url_quote("a b+c~d"), "a%20b%2Bc~d");
    }

    #[test]
    fn client_sends_headers_and_api_base() {
        let transport = FakeTransport::new(vec![ok(json!({"id": 1, "username": "octo"}))]);
        let client = GitLabClient::new("token".to_string(), "https://gitlab.com", &transport)
            .expect("client");
        assert_eq!(client.api_base(), "https://gitlab.com/api/v4");
        let user = client.get_authenticated_user().expect("user");
        assert_eq!(user["username"], json!("octo"));
        let calls = transport.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "GET");
        assert_eq!(calls[0].url, "https://gitlab.com/api/v4/user");
        assert!(calls[0].query.is_empty());
        let headers = &calls[0].headers;
        assert!(headers.contains(&("PRIVATE-TOKEN".to_string(), "token".to_string())));
        assert!(headers.contains(&("Accept".to_string(), "application/json".to_string())));
        assert!(headers.contains(&("User-Agent".to_string(), "pi-dash-gitlab-sync".to_string())));
        assert_eq!(calls[0].timeout_secs, DEFAULT_TIMEOUT_SECONDS);
    }

    #[test]
    fn client_empty_token_is_auth_error() {
        let transport = FakeTransport::new(vec![]);
        let err = match GitLabClient::new(String::new(), "https://gitlab.com", &transport) {
            Ok(_) => panic!("empty token must fail"),
            Err(err) => err,
        };
        assert_eq!(
            err,
            GitProviderError::Auth("GitLab token is missing".into())
        );
    }

    #[test]
    fn client_rejects_off_host_absolute_url() {
        let transport = FakeTransport::new(vec![]);
        let client =
            GitLabClient::new("t".to_string(), "https://gitlab.com", &transport).expect("client");
        assert_eq!(
            client
                .request(
                    "GET",
                    "https://evil.example.com/api/v4/user",
                    vec![],
                    vec![]
                )
                .unwrap_err(),
            GitProviderError::Permission(
                "GitLab API request target does not match the configured host".into()
            )
        );
        assert!(transport.calls().is_empty());
        // Same-host absolute URLs pass through verbatim.
        let transport = FakeTransport::new(vec![ok(json!({}))]);
        let client =
            GitLabClient::new("t".to_string(), "https://gitlab.com", &transport).expect("client");
        client
            .request("GET", "https://gitlab.com/api/v4/user", vec![], vec![])
            .expect("same host");
        assert_eq!(transport.calls()[0].url, "https://gitlab.com/api/v4/user");
    }

    #[test]
    fn client_status_mapping() {
        for (status, error) in [
            (
                302u16,
                GitProviderError::Permission("GitLab API redirects are not followed".into()),
            ),
            (401, GitProviderError::Auth("bad".into())),
            (403, GitProviderError::Permission("denied".into())),
            (404, GitProviderError::NotFound("gone".into())),
            // `raise_for_status` fallthrough renders 400 in the views.
            (422, GitProviderError::General("oops".into())),
            (500, GitProviderError::General("boom".into())),
        ] {
            let transport = FakeTransport::new(vec![status_response(status, "oops-body")]);
            let client =
                GitLabClient::new("t".to_string(), "https://gitlab.com", &transport).expect("c");
            let err = client.request("GET", "/user", vec![], vec![]).unwrap_err();
            let expected = match error {
                GitProviderError::Permission(_) if status == 302 => error,
                GitProviderError::Auth(_) => GitProviderError::Auth("oops-body".into()),
                GitProviderError::Permission(_) => GitProviderError::Permission("oops-body".into()),
                GitProviderError::NotFound(_) => GitProviderError::NotFound("oops-body".into()),
                GitProviderError::General(_) => GitProviderError::General("oops-body".into()),
            };
            assert_eq!(err, expected, "status {status}");
        }
    }

    #[test]
    fn paginate_defaults_and_next_page() {
        let transport = FakeTransport::new(vec![
            GitLabResponse {
                status: 200,
                headers: vec![("X-Next-Page".to_string(), "2".to_string())],
                body: "[{\"id\":1}]".to_string(),
            },
            ok(json!([{"id": 2}])),
        ]);
        let client =
            GitLabClient::new("t".to_string(), "https://gitlab.com", &transport).expect("client");
        let items = client.paginate("/projects/1/issues", &[]).expect("pages");
        assert_eq!(items, vec![json!({"id": 1}), json!({"id": 2})]);
        let calls = transport.calls();
        assert_eq!(calls.len(), 2);
        assert!(calls[0]
            .query
            .contains(&("per_page".to_string(), "100".to_string())));
        assert!(calls[0]
            .query
            .contains(&("page".to_string(), "1".to_string())));
        assert!(calls[1]
            .query
            .contains(&("page".to_string(), "2".to_string())));
    }

    #[test]
    fn has_next_header_case_insensitive() {
        let lower = GitLabResponse {
            status: 200,
            headers: vec![("x-next-page".to_string(), "3".to_string())],
            body: "[]".to_string(),
        };
        assert!(GitLabClient::<FakeTransport>::has_next(&lower));
        assert!(!GitLabClient::<FakeTransport>::has_next(&ok(json!([]))));
    }

    #[test]
    fn list_projects_params() {
        let transport = FakeTransport::new(vec![ok(json!([]))]);
        let client =
            GitLabClient::new("t".to_string(), "https://gitlab.com", &transport).expect("client");
        let (projects, has_next) = client.list_projects(2, 50).expect("projects");
        assert!(projects.is_empty() && !has_next);
        let query = &transport.calls()[0].query;
        // `requests` renders bools `True`, not `true`.
        assert!(query.contains(&("membership".to_string(), "True".to_string())));
        assert!(query.contains(&("simple".to_string(), "True".to_string())));
        assert!(query.contains(&("order_by".to_string(), "last_activity_at".to_string())));
        assert!(query.contains(&("sort".to_string(), "desc".to_string())));
        assert!(query.contains(&("page".to_string(), "2".to_string())));
        assert!(query.contains(&("per_page".to_string(), "50".to_string())));
    }

    #[test]
    fn project_paths_quote_ids() {
        let transport = FakeTransport::new(vec![
            ok(json!({"id": 1})),
            ok(json!([{"id": 7}])),
            ok(json!([{"id": 9}])),
            ok(json!({"id": 3})),
            ok(json!({"id": 4})),
        ]);
        let client =
            GitLabClient::new("t".to_string(), "https://gitlab.com", &transport).expect("client");
        client.get_project("platform/backend/api").expect("project");
        client
            .list_open_issues("platform/backend/api")
            .expect("issues");
        client.list_issue_notes("1", "7").expect("notes");
        client.post_issue_note("1", "7", "hi").expect("note");
        client.get_merge_request("1", "9").expect("mr");
        let urls: Vec<String> = transport.calls().iter().map(|c| c.url.clone()).collect();
        assert_eq!(
            urls,
            vec![
                "https://gitlab.com/api/v4/projects/platform%2Fbackend%2Fapi".to_string(),
                "https://gitlab.com/api/v4/projects/platform%2Fbackend%2Fapi/issues".to_string(),
                "https://gitlab.com/api/v4/projects/1/issues/7/notes".to_string(),
                "https://gitlab.com/api/v4/projects/1/issues/7/notes".to_string(),
                "https://gitlab.com/api/v4/projects/1/merge_requests/9".to_string(),
            ]
        );
        assert_eq!(transport.calls()[2].method, "GET");
        assert_eq!(transport.calls()[3].method, "POST");
        assert_eq!(
            transport.calls()[3].form,
            vec![("body".to_string(), "hi".to_string())]
        );
        // Issue notes paginate oldest-first; issues newest-first.
        assert!(transport.calls()[1]
            .query
            .contains(&("state".to_string(), "opened".to_string())));
        assert!(transport.calls()[2]
            .query
            .contains(&("sort".to_string(), "asc".to_string())));
    }

    #[test]
    fn adapter_keys_and_capabilities_replay_golden() {
        let golden = gitlab_golden();
        assert_eq!(golden["adapter_keys"]["key"], json!("gitlab"));
        assert_eq!(golden["adapter_keys"]["display_name"], json!("GitLab"));
        assert_eq!(
            golden["adapter_keys"]["code_review_term"],
            json!("merge request")
        );
        assert_eq!(GitLabAdapter::<FakeTransport>::KEY, "gitlab");
        assert_eq!(GitLabAdapter::<FakeTransport>::DISPLAY_NAME, "GitLab");
        assert_eq!(
            GitLabAdapter::<FakeTransport>::CODE_REVIEW_TERM,
            "merge request"
        );

        let transport = FakeTransport::new(vec![]);
        let adapter = GitLabAdapter::new(&transport);
        let caps = adapter
            .credential_capabilities(&json!({}))
            .expect("capabilities");
        let replayed = serde_json::to_value(caps).expect("value");
        assert_eq!(replayed, golden["capability_map"]["any_credential"]);
    }

    #[test]
    fn token_missing_replays_golden() {
        let golden = gitlab_golden();
        assert_eq!(
            golden["token_missing"]["error"],
            json!("GitProviderAuthError: GitLab token is missing")
        );
        assert_eq!(
            GitLabAdapter::<FakeTransport>::resolve_token(&json!({})).unwrap_err(),
            GitProviderError::Auth("GitLab token is missing".into())
        );
        assert_eq!(
            GitLabAdapter::<FakeTransport>::resolve_token(&json!({"token": ""})).unwrap_err(),
            GitProviderError::Auth("GitLab token is missing".into())
        );
        // Plaintext passthrough by default.
        assert_eq!(
            GitLabAdapter::<FakeTransport>::resolve_token(&json!({"token": "raw"})).expect("t"),
            "raw"
        );
    }

    #[test]
    fn token_decrypt_or_raw() {
        fn failing(_: &str) -> Option<String> {
            None
        }
        fn empty(_: &str) -> Option<String> {
            Some(String::new())
        }
        fn good(_: &str) -> Option<String> {
            Some("decrypted".to_string())
        }
        let cred = json!({"token": "stored"});
        assert_eq!(
            GitLabAdapter::<FakeTransport>::resolve_token_with(&cred, &failing).expect("t"),
            "stored"
        );
        assert_eq!(
            GitLabAdapter::<FakeTransport>::resolve_token_with(&cred, &empty).expect("t"),
            "stored"
        );
        assert_eq!(
            GitLabAdapter::<FakeTransport>::resolve_token_with(&cred, &good).expect("t"),
            "decrypted"
        );
    }

    fn self_managed(configured: &str) -> (GitLabConfig, FakeTransport) {
        (
            GitLabConfig {
                gitlab_host: String::new(),
                allowed_hosts: vec![configured.to_string()],
            },
            FakeTransport::new(vec![]),
        )
    }

    #[test]
    fn host_gates_reject_before_http() {
        let (config, transport) = self_managed("https://gitlab.example.com");
        let adapter = GitLabAdapter::with_config(config, &transport);
        // Disallowed host: no round trip.
        assert_eq!(
            adapter
                .verify_provider_account(
                    &json!({"token": "t", "host_url": "https://evil.example.com"})
                )
                .unwrap_err()
                .to_string(),
            "GitLab host is not allowed; ask an instance admin to add it to GITLAB_ALLOWED_HOSTS."
        );
        // Plain HTTP: rejected even though the host string is allowlisted
        // in spirit — the HTTPS gate runs first.
        assert_eq!(
            adapter
                .verify_provider_account(
                    &json!({"token": "t", "host_url": "http://gitlab.example.com"})
                )
                .unwrap_err(),
            GitProviderError::Permission("GitLab host must use HTTPS".into())
        );
        // Metadata-loopback style hosts are not allowlisted by default.
        for host in [
            "http://localhost:8080",
            "http://169.254.169.254",
            "https://evil.example.com",
        ] {
            assert!(adapter
                .verify_provider_account(&json!({"token": "t", "host_url": host}))
                .is_err());
        }
        assert!(transport.calls().is_empty());
    }

    #[test]
    fn verify_uses_configured_host_and_normalizes() {
        let transport = FakeTransport::new(vec![ok(json!({"id": 2, "username": "self-managed"}))]);
        let (config, _) = self_managed("https://gitlab.example.com");
        let adapter = GitLabAdapter::with_config(config, &transport);
        // Bare hostnames normalize before the allowlist lookup.
        let identity = adapter
            .verify_provider_account(&json!({"token": "t", "host_url": "gitlab.example.com"}))
            .expect("identity");
        assert_eq!(identity["username"], json!("self-managed"));
        assert_eq!(
            transport.calls()[0].url,
            "https://gitlab.example.com/api/v4/user"
        );
        // Credential without `host_url` falls back to `GITLAB_HOST`.
        let transport = FakeTransport::new(vec![ok(json!({"id": 3}))]);
        let adapter = GitLabAdapter::with_config(
            GitLabConfig {
                gitlab_host: "gitlab.example.com".to_string(),
                allowed_hosts: vec![],
            },
            &transport,
        );
        adapter
            .verify_provider_account(&json!({"token": "t"}))
            .expect("identity");
        assert_eq!(
            transport.calls()[0].url,
            "https://gitlab.example.com/api/v4/user"
        );
    }

    #[test]
    fn parse_repo_url_replays_golden() {
        let golden = gitlab_golden();
        let cases = &golden["parse_repo_url"];
        let transport = FakeTransport::new(vec![]);
        let (config, _) = self_managed("https://gitlab.example.com");
        let adapter = GitLabAdapter::with_config(config, &transport);

        let ssh = adapter
            .parse_repo_url("git@gitlab.example.com:platform/backend/api.git")
            .expect("ssh subgroup");
        assert_eq!(ssh.provider, "gitlab");
        assert_eq!(ssh.host_url, "https://gitlab.example.com");
        assert_eq!(ssh.namespace, "platform/backend");
        assert_eq!(ssh.name, "api");
        assert_eq!(ssh.full_name, "platform/backend/api");
        assert_eq!(
            ssh.clone_url,
            "git@gitlab.example.com:platform/backend/api.git"
        );
        assert_eq!(
            ssh.full_name,
            cases["ok_ssh_subgroup"]["full_name"].as_str().unwrap()
        );
        assert_eq!(
            ssh.namespace,
            cases["ok_ssh_subgroup"]["namespace"].as_str().unwrap()
        );

        let https = adapter
            .parse_repo_url("https://gitlab.example.com/acme/web.git")
            .expect("https .git");
        assert_eq!(https.full_name, "acme/web");
        assert_eq!(
            https.full_name,
            cases["ok_https_git_suffix_stripped"]["full_name"]
                .as_str()
                .unwrap()
        );

        // `/-/` view suffixes are cut before the split.
        let tree = adapter
            .parse_repo_url("https://gitlab.example.com/platform/backend/api/-/tree/main")
            .expect("dashdash suffix");
        assert_eq!(tree.full_name, "platform/backend/api");
        assert_eq!(
            tree.full_name,
            cases["ok_https_dashdash_suffix"]["full_name"]
                .as_str()
                .unwrap()
        );

        // `ssh://` URLs keep working with userinfo stripped for the gate.
        let via_ssh = adapter
            .parse_repo_url("ssh://git@gitlab.example.com/platform/backend/api.git")
            .expect("ssh url");
        assert_eq!(via_ssh.full_name, "platform/backend/api");

        assert!(adapter
            .parse_repo_url("https://evil.example.com/acme/web")
            .is_none());
        assert!(adapter
            .parse_repo_url("ftp://gitlab.example.com/acme/web")
            .is_none());
        assert!(adapter
            .parse_repo_url("https://gitlab.example.com/noslash")
            .is_none());
        assert!(adapter.parse_repo_url("").is_none());
        assert!(cases["evil_host"].is_null());
        assert!(cases["ftp_scheme"].is_null());
        assert!(cases["no_slash"].is_null());
    }

    #[test]
    fn parse_code_review_url_replays_golden() {
        let golden = gitlab_golden();
        let cases = &golden["parse_code_review_url"];
        let transport = FakeTransport::new(vec![]);
        let (config, _) = self_managed("https://gitlab.example.com");
        let adapter = GitLabAdapter::with_config(config, &transport);

        let review = adapter
            .parse_code_review_url(
                "https://gitlab.example.com/platform/backend/api/-/merge_requests/17",
            )
            .expect("mr url");
        let expected = &cases["ok"];
        assert_eq!(review.provider, "gitlab");
        assert_eq!(review.host_url, expected["host_url"].as_str().unwrap());
        assert_eq!(review.namespace, expected["namespace"].as_str().unwrap());
        assert_eq!(review.repo_name, expected["repo_name"].as_str().unwrap());
        assert_eq!(
            review.external_iid,
            expected["external_iid"].as_str().unwrap()
        );
        assert_eq!(review.url, expected["url"].as_str().unwrap());

        assert!(adapter
            .parse_code_review_url("https://evil.example.com/a/b/-/merge_requests/1")
            .is_none());
        assert!(adapter
            .parse_code_review_url("https://gitlab.example.com/a/b/-/issues/1")
            .is_none());
        assert!(adapter
            .parse_code_review_url("https://gitlab.example.com/a/b/-/merge_requests/abc")
            .is_none());
        assert!(adapter
            .parse_code_review_url("https://gitlab.example.com/a/b/-/merge_requests/")
            .is_none());
    }

    #[test]
    fn remote_repo_visibility_replays_golden() {
        let golden = gitlab_golden()["remote_repo_visibility"].clone();
        assert!(golden["private_visibility_true"].as_bool().unwrap());
        assert!(!golden["public_visibility_false"].as_bool().unwrap());

        let private = GitLabAdapter::<FakeTransport>::remote_repo(&json!({
            "id": 1,
            "path_with_namespace": "platform/backend/api",
            "path": "api",
            "web_url": "https://gitlab.example.com/platform/backend/api",
            "http_url_to_repo": "https://gitlab.example.com/platform/backend/api.git",
            "ssh_url_to_repo": "git@gitlab.example.com:platform/backend/api.git",
            "default_branch": "main",
            "visibility": "private",
        }));
        assert!(private.is_private);
        assert_eq!(private.namespace, "platform/backend");
        assert_eq!(private.name, "api");
        assert_eq!(private.external_id, "1");

        let public = GitLabAdapter::<FakeTransport>::remote_repo(&json!({
            "id": 2,
            "path": "solo",
            "visibility": "public",
        }));
        assert!(!public.is_private);
        // No slash in the combined name: the `("", path)` fallback.
        assert_eq!(public.namespace, "");
        assert_eq!(public.name, "solo");
        assert_eq!(public.full_name, "solo");
    }

    #[test]
    fn list_and_get_repositories() {
        let transport = FakeTransport::new(vec![
            ok(json!([{
                "id": 7,
                "path_with_namespace": "acme/web",
                "visibility": "private",
                "web_url": "https://gitlab.com/acme/web",
            }])),
            ok(json!({
                "id": 7,
                "path_with_namespace": "acme/web",
                "visibility": "private",
                "web_url": "https://gitlab.com/acme/web",
            })),
        ]);
        let adapter = GitLabAdapter::new(&transport);
        let cred = json!({"token": "t", "host_url": "https://gitlab.com"});
        let page = adapter.list_repositories(&cred, 1).expect("page");
        assert_eq!(page.page, 1);
        assert!(!page.has_next_page);
        assert_eq!(page.repositories.len(), 1);
        assert!(page.repositories[0].is_private);
        let parsed = adapter
            .parse_repo_url("https://gitlab.com/acme/web")
            .expect("parsed");
        let repo = adapter.get_repository(&cred, &parsed).expect("repo");
        assert_eq!(repo.full_name, "acme/web");
        // `get_project` quotes the full path (`/` -> `%2F`).
        assert_eq!(
            transport.calls()[1].url,
            "https://gitlab.com/api/v4/projects/acme%2Fweb"
        );
    }

    fn example_repository() -> RemoteRepository {
        RemoteRepository {
            provider: "gitlab".into(),
            external_id: "7".into(),
            namespace: "acme".into(),
            name: "web".into(),
            full_name: "acme/web".into(),
            web_url: "https://gitlab.com/acme/web".into(),
            clone_url_http: String::new(),
            clone_url_ssh: String::new(),
            default_branch: "main".into(),
            is_private: false,
            metadata: json!({}),
        }
    }

    #[test]
    fn list_open_issues_mapping() {
        let transport = FakeTransport::new(vec![ok(json!([{
            "id": 1001,
            "iid": 7,
            "title": "Upstream title",
            "description": "Upstream **body**",
            "state": "opened",
            "author": {"username": "octo"},
            "web_url": "https://gitlab.example.com/acme/web/-/issues/7",
            "created_at": "2024-01-02T03:04:05.000Z",
            "updated_at": "2024-01-02T04:00:00Z",
        }]))]);
        let (config, _) = self_managed("https://gitlab.example.com");
        let adapter = GitLabAdapter::with_config(config, &transport);
        let cred = json!({"token": "t", "host_url": "https://gitlab.example.com"});
        let issues = adapter
            .list_open_issues(&cred, &example_repository())
            .expect("issues");
        assert_eq!(issues.len(), 1);
        let issue = &issues[0];
        assert_eq!(issue.external_id, "1001");
        assert_eq!(issue.external_iid, "7");
        assert_eq!(issue.title, "Upstream title");
        assert_eq!(issue.body, "Upstream **body**");
        assert_eq!(issue.state, "opened");
        assert_eq!(issue.author, "octo");
        assert_eq!(
            issue.created_at.as_deref(),
            Some("2024-01-02T03:04:05+00:00")
        );
        assert_eq!(
            issue.updated_at.as_deref(),
            Some("2024-01-02T04:00:00+00:00")
        );
        // Full provider payload rides along verbatim.
        assert_eq!(issues[0].metadata["id"], json!(1001));
        // Project selected by `external_id`.
        assert!(transport.calls()[0]
            .url
            .starts_with("https://gitlab.example.com/api/v4/projects/7/issues"));
    }

    #[test]
    fn list_open_issues_falls_back_to_full_name() {
        let transport = FakeTransport::new(vec![ok(json!([]))]);
        let adapter = GitLabAdapter::new(&transport);
        let mut repo = example_repository();
        repo.external_id = String::new();
        repo.full_name = "acme/web".to_string();
        adapter
            .list_open_issues(
                &json!({"token": "t", "host_url": "https://gitlab.com"}),
                &repo,
            )
            .expect("issues");
        assert_eq!(
            transport.calls()[0].url,
            "https://gitlab.com/api/v4/projects/acme%2Fweb/issues"
        );
    }

    #[test]
    fn list_issue_comments_skips_system_and_blanks_url() {
        let golden = gitlab_golden();
        assert!(golden["list_issue_notes"]["system_notes_skipped"]
            .as_bool()
            .unwrap());
        assert!(golden["list_issue_notes"]["web_url_always_empty_string"]
            .as_bool()
            .unwrap());
        let transport = FakeTransport::new(vec![ok(json!([
            {"id": 1, "system": true, "body": "changed milestone", "author": {"username": "bot"}},
            {
                "id": 2,
                "system": false,
                "body": "Nice fix",
                "author": {"username": "octo"},
                "created_at": "2024-01-02T03:04:05Z",
                "updated_at": "2024-01-02T03:05:00Z",
            },
        ]))]);
        let adapter = GitLabAdapter::new(&transport);
        let comments = adapter
            .list_issue_comments(
                &json!({"token": "t", "host_url": "https://gitlab.com"}),
                &example_repository(),
                "7",
            )
            .expect("comments");
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].external_id, "2");
        assert_eq!(comments[0].author, "octo");
        assert_eq!(comments[0].web_url, "");
    }

    #[test]
    fn post_issue_comment_mapping() {
        let transport = FakeTransport::new(vec![ok(json!({
            "id": 3,
            "body": "hi",
            "author": {"username": "octo"},
            "created_at": "2024-01-02T03:04:05Z",
        }))]);
        let adapter = GitLabAdapter::new(&transport);
        let comment = adapter
            .post_issue_comment(
                &json!({"token": "t", "host_url": "https://gitlab.com"}),
                &example_repository(),
                "7",
                "hi",
            )
            .expect("comment");
        assert_eq!(comment.external_id, "3");
        assert_eq!(comment.body, "hi");
        assert_eq!(comment.web_url, "");
        assert_eq!(
            transport.calls()[0].form,
            vec![("body".to_string(), "hi".to_string())]
        );
    }

    #[test]
    fn get_code_review_state_table_replays_golden() {
        let golden = gitlab_golden()["get_code_review_state"].clone();
        let transport = FakeTransport::new(vec![
            ok(json!({"id": 1, "iid": 9, "title": "T", "state": "merged"})),
            ok(json!({"id": 2, "iid": 9, "title": "T", "state": "closed"})),
            ok(json!({"id": 3, "iid": 9, "title": "T", "state": "opened"})),
            ok(json!({"id": 4, "iid": 9, "title": "T", "state": "locked"})),
            ok(json!({"id": 5, "iid": 9, "title": "T"})),
        ]);
        let adapter = GitLabAdapter::new(&transport);
        let cred = json!({"token": "t", "host_url": "https://gitlab.com"});
        let parsed = ParsedCodeReview {
            provider: "gitlab".into(),
            host_url: "https://gitlab.com".into(),
            namespace: "acme".into(),
            repo_name: "web".into(),
            external_iid: "9".into(),
            url: "https://gitlab.com/acme/web/-/merge_requests/9".into(),
        };
        for (raw, expected) in [
            ("merged", "merged"),
            ("closed", "closed"),
            ("opened", "open"),
            ("locked", "open"),
        ] {
            let review = adapter.get_code_review(&cred, &parsed).expect("review");
            assert_eq!(review.state, expected, "raw state {raw}");
            assert_eq!(review.state, golden[raw].as_str().unwrap());
            assert_eq!(review.merged, raw == "merged");
        }
        // Missing state reads as `"opened"` -> `"open"`.
        let review = adapter.get_code_review(&cred, &parsed).expect("review");
        assert_eq!(review.state, "open");
        assert_eq!(review.state, golden["empty"].as_str().unwrap());
    }

    #[test]
    fn get_code_review_draft_and_fallbacks() {
        // `draft` truthy wins; strict `work_in_progress is True` also wins.
        for (payload, draft) in [
            (
                json!({"id": 1, "iid": 9, "state": "opened", "draft": true}),
                true,
            ),
            (
                json!({"id": 1, "iid": 9, "state": "opened", "draft": "yes"}),
                true,
            ),
            (
                json!({"id": 1, "iid": 9, "state": "opened", "work_in_progress": true}),
                true,
            ),
            (
                json!({"id": 1, "iid": 9, "state": "opened", "work_in_progress": 1}),
                false,
            ),
            (json!({"id": 1, "iid": 9, "state": "opened"}), false),
        ] {
            let transport = FakeTransport::new(vec![ok(payload)]);
            let adapter = GitLabAdapter::new(&transport);
            let review = adapter
                .get_code_review(
                    &json!({"token": "t", "host_url": "https://gitlab.com"}),
                    &ParsedCodeReview {
                        provider: "gitlab".into(),
                        host_url: "https://gitlab.com".into(),
                        namespace: "acme".into(),
                        repo_name: "web".into(),
                        external_iid: "9".into(),
                        url: "https://gitlab.com/acme/web/-/merge_requests/9".into(),
                    },
                )
                .expect("review");
            assert_eq!(review.draft, draft, "payload {}", review.metadata);
        }
        // `web_url` falls back to the parsed URL; `iid` falls back to the
        // parsed iid; `updated_at` renders through `_parse_dt`.
        let transport = FakeTransport::new(vec![ok(json!({
            "id": 11,
            "state": "opened",
            "title": "Add widget",
            "updated_at": "2024-01-02T03:04:05Z",
        }))]);
        let adapter = GitLabAdapter::new(&transport);
        let review = adapter
            .get_code_review(
                &json!({"token": "t", "host_url": "https://gitlab.com"}),
                &ParsedCodeReview {
                    provider: "gitlab".into(),
                    host_url: "https://gitlab.com".into(),
                    namespace: "acme".into(),
                    repo_name: "web".into(),
                    external_iid: "9".into(),
                    url: "https://gitlab.com/acme/web/-/merge_requests/9".into(),
                },
            )
            .expect("review");
        assert_eq!(review.external_id, "11");
        assert_eq!(review.external_iid, "9");
        assert_eq!(
            review.web_url,
            "https://gitlab.com/acme/web/-/merge_requests/9"
        );
        assert_eq!(
            review.updated_at.as_deref(),
            Some("2024-01-02T03:04:05+00:00")
        );
    }

    #[test]
    fn normalize_webhook_issue_hook() {
        let payload = json!({
            "event_type": "issue",
            "project": {"id": 1, "path_with_namespace": "acme/web"},
            "object_attributes": {"id": 3, "action": "update"},
        });
        let event = GitLabAdapter::<FakeTransport>::normalize_webhook(
            serde_json::to_string(&payload).unwrap().as_bytes(),
            &json!({"X-Gitlab-Event": "Issue Hook"}),
        )
        .expect("event");
        assert_eq!(event.provider, "gitlab");
        assert_eq!(event.event, "Issue Hook");
        assert_eq!(event.action, "update");
        assert_eq!(event.repository_ref["id"], json!(1));
        assert_eq!(event.issue_ref["id"], json!(3));
        assert!(event.code_review_ref.as_object().unwrap().is_empty());
        assert_eq!(event.payload, payload);
    }

    #[test]
    fn normalize_webhook_merge_request_hook_bug_ported() {
        // A real `Merge Request Hook` header never contains the
        // `"merge_request"` substring (spaces vs underscore), so
        // `code_review_ref` is `{}` — ported verbatim, not fixed.
        let payload = json!({
            "event_type": "merge_request",
            "object_attributes": {"id": 9, "action": "open"},
        });
        let event = GitLabAdapter::<FakeTransport>::normalize_webhook(
            serde_json::to_string(&payload).unwrap().as_bytes(),
            &json!({"X-GitLab-Event": "Merge Request Hook"}),
        )
        .expect("event");
        assert_eq!(event.event, "Merge Request Hook");
        assert_eq!(event.action, "open");
        assert!(event.code_review_ref.as_object().unwrap().is_empty());
        assert!(event.issue_ref.as_object().unwrap().is_empty());
        // The underscored spelling (as some proxies forward it) does match.
        let event = GitLabAdapter::<FakeTransport>::normalize_webhook(
            serde_json::to_string(&payload).unwrap().as_bytes(),
            &json!({"X-Gitlab-Event": "merge_request"}),
        )
        .expect("event");
        assert_eq!(event.code_review_ref["id"], json!(9));
    }

    #[test]
    fn normalize_webhook_empty_and_invalid_bodies() {
        let event = GitLabAdapter::<FakeTransport>::normalize_webhook(b"", &json!({})).expect("e");
        assert_eq!(event.event, "");
        assert_eq!(event.action, "");
        assert_eq!(event.payload, json!({}));
        assert!(GitLabAdapter::<FakeTransport>::normalize_webhook(b"nope", &json!({})).is_err());
        assert!(
            GitLabAdapter::<FakeTransport>::normalize_webhook(&[0xff, 0xfe], &json!({})).is_err()
        );
    }

    #[test]
    fn request_guards_replay_golden() {
        // Fixture `request_guards`: api_base, per_page default, redirect
        // and host-mismatch guards. Covered by the client tests above; this
        // pins the golden vocabulary itself.
        let golden = gitlab_golden()["request_guards"].clone();
        assert_eq!(golden["api_base"], json!("<host>/api/v4"));
        assert_eq!(golden["per_page_default"], json!(100));
        assert!(golden["allow_redirects_false"].as_bool().unwrap());
    }

    #[test]
    fn adapter_satisfies_provider_trait() {
        // The contract is used through concrete types (it carries
        // associated consts, so it is not `dyn`-compatible); a generic
        // bound still proves every member resolves.
        fn via_trait<A: GitProviderAdapter>(adapter: &A) -> bool {
            A::KEY == "gitlab"
                && adapter
                    .parse_repo_url("https://gitlab.com/acme/web")
                    .is_some()
                && adapter
                    .parse_repo_url("https://evil.example.com/a/b")
                    .is_none()
        }
        let transport = FakeTransport::new(vec![]);
        let adapter = GitLabAdapter::new(&transport);
        assert!(via_trait(&adapter));
        assert_eq!(
            GitLabAdapter::<FakeTransport>::CODE_REVIEW_TERM,
            "merge request"
        );
    }
}
