//! GitHub git-provider adapter (D-05, stage 5).
//!
//! Ports `apps/api/pi_dash/integrations/git/adapters/github.py:41-257`
//! (`GitHubAdapter`, key `"github"`): `_parse_dt` (41-48), `parse_repo_url`
//! (55-70), `parse_code_review_url` (71-86), `_token`/`_client` (87-99),
//! `_map_error` (101-109), `verify_provider_account` (110-115),
//! `credential_capabilities` (116-125), `_remote_repo` (126-144),
//! `list_repositories` (145-155), `get_repository` (156-161),
//! `list_open_issues` (162-184), `list_issue_comments` (185-206),
//! `post_issue_comment` (207-229), `get_code_review` (230-248),
//! `normalize_webhook` (249-257).
//!
//! It implements the 14-member [`GitProviderAdapter`] contract from
//! `pidash-types` (`adapters/base.py:23-86`, ported by PIDASHCONV-140).
//!
//! HTTP seam: the services crate holds no HTTP client (no new framework
//! dependency enters the lockfile), so GitHub REST calls cross the
//! synchronous [`GithubClient`] trait — the same blocking shape as Python's
//! `GithubClient` (`utils/github_client.py:38-220`). The adapter resolves
//! the credential to a [`ClientAuth`] (PAT token or installation id,
//! mirroring `_client`) and builds the transport via
//! `GithubClient::connect`. A real HTTP transport lands with the tasks
//! layer; unit tests inject scripted fakes.
//!
//! Datetimes cross the DTO boundary as pre-rendered ISO-8601 strings
//! (`Option<String>`, the `pidash-types` convention): [`parse_dt`]
//! normalizes exactly what `_parse_dt` accepts and renders what Python's
//! `datetime.isoformat()` would emit, so JSON stays byte-identical.
//!
//! Fixture: `rust-api/fixtures/integrations/adapters/github.golden.json`.
//!
//! Ported bugs / inherited semantics (translate, don't redesign):
//!
//! * A string (or other non-mapping) `owner` in a repo payload raises in
//!   Python (`'str' object has no attribute 'get'`); a mapping `owner`
//!   without a usable string `login` falls through to `.lower()` on the
//!   mapping and raises too. Both surface here as
//!   `GitProviderError::General` with the same message shape (a
//!   non-provider error, like the original `AttributeError`, which
//!   `_map_error` passes through unchanged).
//! * `get_code_review` renders `"merged"` whenever the snapshot says so,
//!   even when the PR `state` is `"closed"`; any non-`"closed"` snapshot
//!   state (including missing) becomes `"open"` (`pr_snapshot_from_payload`
//!   over `utils/github_client.py:287-300`).
//! * `list_open_issues` skips by `pull_request` key *presence*, whatever
//!   its value.
//! * `parse_repo_url` lowercases `owner`/`name` but keeps `clone_url` as
//!   the raw stripped input; `parse_code_review_url` rebuilds the URL in
//!   canonical form with `str(int(number))` (leading zeros stripped).
//! * `credential_capabilities` keys off `auth_type` with a `"pat"`
//!   default; `clone` is always `false`.
//! * `_token` falls back to the raw token when decryption fails closed
//!   (the golden's `token_missing.source` note); an empty token raises
//!   `GitProviderError::Auth("GitHub token is missing")`.
//!
//! Deliberate approximations (unreachable with real GitHub payloads,
//! documented here instead of a paragraph per call site):
//!
//! * Non-string JSON scalars in string DTO fields coerce via Python
//!   truthiness (`0`/`""`/`false`/`null` render `""`; `true` renders
//!   `"True"`; other numbers render as-is); containers render `""`.
//!   Python would keep (or crash on) such values, but GitHub always sends
//!   strings here.
//! * `_parse_dt` covers RFC 3339 plus naive `YYYY-MM-DDTHH:MM:SS[.ffffff]`
//!   (either separator) plus date-only midnight, rendered with exactly six
//!   fractional digits when nonzero — the shapes GitHub emits. Sub-second
//!   precision beyond microseconds truncates, like `fromisoformat`.
//!   Exotic `fromisoformat` spellings (offsets without colons, week dates)
//!   yield `None`.
//! * `int(issue_iid)` covers optional sign plus ASCII digits with
//!   surrounding whitespace stripped. Underscore digit separators and
//!   non-ASCII digits yield an error where Python would accept.
//! * `normalize_webhook` maps UTF-8/JSON decode failures to
//!   `GitProviderError::General`; Python would raise `UnicodeDecodeError`
//!   / `JSONDecodeError` (both non-provider errors, like `General` here).

use std::marker::PhantomData;

use chrono::{DateTime, NaiveDate, NaiveDateTime};
use pidash_db::config::encryption::Keyring;
use pidash_types::integrations::{
    GitProviderAdapter, GitProviderCapabilities, GitProviderError, ParsedCodeReview,
    ParsedRepository, ProviderWebhookEvent, RemoteCodeReview, RemoteComment, RemoteIssue,
    RemoteRepository, RepositoryPage,
};
use serde_json::Value;

/// Canonical host (`github.py:38`). `parse_repo_url` always reports this;
/// `parse_code_review_url` rebuilds review URLs under it.
pub const GITHUB_HOST: &str = "https://github.com";

/// GitHub-side failure (`utils/github_client.py:26-34`).
///
/// `Auth` / `Permission` / `NotFound` mirror `GithubAuthError` (HTTP 401),
/// `GithubPermissionError` (HTTP 403) and `GithubNotFoundError` (HTTP 404).
/// `Transport` carries any other client failure (e.g. the `HTTPError` from
/// `response.raise_for_status()` on 5xx); like Python's `_map_error`, which
/// returns such exceptions unchanged, it maps to the base
/// [`GitProviderError::General`] with the message preserved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubError {
    Auth(String),
    Permission(String),
    NotFound(String),
    Transport(String),
}

impl GithubError {
    /// The `str(exc)` message, preserved verbatim across mapping.
    pub fn message(&self) -> &str {
        match self {
            GithubError::Auth(msg)
            | GithubError::Permission(msg)
            | GithubError::NotFound(msg)
            | GithubError::Transport(msg) => msg,
        }
    }
}

/// How the adapter authenticates one transport (`github.py:92-96`).
///
/// `Token` is the decrypted PAT; `Installation` is the GitHub App
/// installation id minted into a token by `installation_token`
/// (`github_client.py:47-52`, owned by the transport implementation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientAuth {
    Token(String),
    Installation(i64),
}

/// Synchronous GitHub REST seam (`GithubClient`, `github_client.py:38-220`).
///
/// Only the endpoints the adapter touches are ported; each returns decoded
/// JSON (`dict` in Python, [`Value`] here). `page` is 1-based
/// (`list_user_repos(page=page)`); `per_page` stays the client's default
/// (100), as in Python.
pub trait GithubClient: Sized {
    /// Build a client for `auth` (`GithubClient(token=…)` /
    /// `GithubClient.for_installation(…)`).
    fn connect(auth: &ClientAuth) -> Result<Self, GithubError>;

    /// `GET /user` — validates a credential on connect (`:99-101`).
    fn get_authenticated_user(&self) -> Result<Value, GithubError>;

    /// One page of `GET /user/repos` (`:103-114`). Returns the page plus
    /// whether a `next` Link header was present.
    fn list_user_repos(&self, page: i64) -> Result<(Vec<Value>, bool), GithubError>;

    /// `GET /repos/{owner}/{repo}` (`:116-119`).
    fn get_repo(&self, owner: &str, name: &str) -> Result<Value, GithubError>;

    /// Paginated `GET /repos/{owner}/{repo}/issues?state=open` (`:133-140`).
    /// PRs arrive alongside issues; the adapter filters them.
    fn list_all_open_issues(&self, owner: &str, name: &str) -> Result<Vec<Value>, GithubError>;

    /// Paginated `GET .../issues/{number}/comments` (`:149-154`).
    fn list_issue_comments(
        &self,
        owner: &str,
        name: &str,
        issue_number: i64,
    ) -> Result<Vec<Value>, GithubError>;

    /// `POST .../issues/{number}/comments` (`:156-160`).
    fn post_issue_comment(
        &self,
        owner: &str,
        name: &str,
        issue_number: i64,
        body: &str,
    ) -> Result<Value, GithubError>;

    /// `GET /repos/{owner}/{repo}/pulls/{number}` (`:162-165`).
    fn get_pull_request(&self, owner: &str, name: &str, number: i64) -> Result<Value, GithubError>;
}

/// `GitHubAdapter` (`github.py:50-53`).
///
/// Holds the Fernet [`Keyring`] used to decrypt stored PATs (mirroring
/// `decrypt_data` over `settings.SECRET_KEY`); the transport type is a
/// parameter so tests inject fakes.
pub struct GitHubAdapter<C> {
    keyring: Keyring,
    transport: PhantomData<C>,
}

impl<C> GitHubAdapter<C> {
    /// Build with an explicit keyring (`Keyring::from_env()` reads the
    /// Django `SECRET_KEY`, like `decrypt_data`).
    pub fn new(keyring: Keyring) -> Self {
        Self {
            keyring,
            transport: PhantomData,
        }
    }
}

/// `_map_error` (`github.py:101-109`): exact provider → provider mapping;
/// anything else passes through as the base error with its message.
pub fn map_github_error(exc: GithubError) -> GitProviderError {
    match exc {
        GithubError::Auth(msg) => GitProviderError::Auth(msg),
        GithubError::Permission(msg) => GitProviderError::Permission(msg),
        GithubError::NotFound(msg) => GitProviderError::NotFound(msg),
        GithubError::Transport(msg) => GitProviderError::General(msg),
    }
}

/// Python truthiness for JSON values (`or`-chain semantics).
fn json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else {
                n.as_f64().is_some_and(|f| f != 0.0)
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(m) => !m.is_empty(),
    }
}

fn json_truthy_opt(value: Option<&Value>) -> bool {
    value.is_some_and(json_truthy)
}

/// `X or ""` for string DTO fields: strings verbatim, `true` as Python's
/// `"True"`, nonzero numbers as-is, everything falsy/missing as `""`
/// (see the module approximations note for containers).
fn py_str_or_empty(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(true)) => "True".to_owned(),
        Some(Value::Bool(false)) => String::new(),
        Some(Value::Number(n)) => {
            if json_truthy(&Value::Number(n.clone())) {
                n.to_string()
            } else {
                String::new()
            }
        }
        Some(Value::Array(_)) | Some(Value::Object(_)) => String::new(),
    }
}

/// `str(X or "")` for id-shaped fields (`str(payload.get("id") or "")`).
/// Identical to [`py_str_or_empty`]: falsy renders `""`, truthy renders
/// Python's `str()`.
fn py_id_or_empty(value: Option<&Value>) -> String {
    py_str_or_empty(value)
}

/// `_parse_dt` (`github.py:41-48`).
///
/// Falsy input yields `None`; otherwise the value is parsed like Python's
/// `datetime.fromisoformat(value.replace("Z", "+00:00"))` and rendered as
/// Python's `datetime.isoformat()`. A `ValueError` yields `None`.
///
/// Accepted shapes (the GitHub surface plus faithful fallbacks): RFC 3339
/// with any colon offset or `Z`; naive `YYYY-MM-DDTHH:MM:SS[.ffffff]` with
/// a `T` or space separator; date-only `YYYY-MM-DD` (midnight, as
/// `fromisoformat` produces). Rendering keeps exactly six fractional digits
/// when nonzero (Python always renders microseconds six-wide) and `+00:00`
/// for UTC (never `Z`).
pub fn parse_dt(value: Option<&str>) -> Option<String> {
    let raw = value?;
    if raw.is_empty() {
        return None;
    }
    // Like Python's `str.replace`: every "Z" occurrence, not just a suffix.
    let normalized = raw.replace('Z', "+00:00");
    if let Ok(parsed) = DateTime::parse_from_rfc3339(&normalized) {
        // `fromisoformat` truncates sub-microsecond precision; rendering
        // floors to microseconds the same way.
        let fraction = render_fraction(parsed.timestamp_subsec_nanos());
        return Some(format!(
            "{}{}{}",
            parsed.format("%Y-%m-%dT%H:%M:%S"),
            fraction,
            parsed.format("%:z")
        ));
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S%.f") {
        return render_naive(naive);
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%d %H:%M:%S%.f") {
        return render_naive(naive);
    }
    if let Ok(date) = NaiveDate::parse_from_str(&normalized, "%Y-%m-%d") {
        if let Some(midnight) = date.and_hms_opt(0, 0, 0) {
            return render_naive(midnight);
        }
    }
    None
}

/// `.ffffff` when microseconds are nonzero, else `""` (Python's
/// `isoformat` omits a zero fraction entirely).
fn render_fraction(nanos: u32) -> String {
    if nanos == 0 {
        String::new()
    } else {
        format!(".{:06}", nanos / 1000)
    }
}

fn render_naive(naive: NaiveDateTime) -> Option<String> {
    let nanos = naive.and_utc().timestamp_subsec_nanos();
    Some(format!(
        "{}{}",
        naive.format("%Y-%m-%dT%H:%M:%S"),
        render_fraction(nanos)
    ))
}

/// Python `str.isspace` coverage for URL segments (`re` `\s` under `str`
/// patterns matches Unicode whitespace).
fn has_whitespace(s: &str) -> bool {
    s.chars().any(char::is_whitespace)
}

/// Shared `owner/name` core of `_HTTPS_REPO_RE` / `_SSH_REPO_RE`
/// (`github_client.py:236-237,240-254`).
///
/// The regexes are lazy with an optional `.git` suffix, so the *shortest*
/// valid name wins: strip one trailing slash for HTTPS (absent for SSH),
/// reject embedded slashes/whitespace, then strip one case-sensitive
/// `.git` suffix when something remains.
fn split_owner_name(rest: &str, allow_trailing_slash: bool) -> Option<(String, String)> {
    let core = if allow_trailing_slash {
        rest.strip_suffix('/').unwrap_or(rest)
    } else {
        rest
    };
    if core.is_empty() || has_whitespace(core) {
        return None;
    }
    let (owner, mut name) = core.split_once('/')?;
    if owner.is_empty() || has_whitespace(owner) {
        return None;
    }
    // The name segment holds no second slash: `[^/\s]+` matches one path
    // segment, and only one trailing slash was stripped above.
    if name.is_empty() || name.contains('/') || has_whitespace(name) {
        return None;
    }
    if name.len() > ".git".len() {
        if let Some(stripped) = name.strip_suffix(".git") {
            name = stripped;
        }
    }
    Some((owner.to_lowercase(), name.to_lowercase()))
}

/// `parse_github_repo_url` (`github_client.py:240-254`): HTTPS
/// (`https://github.com/<owner>/<repo>[.git][/]`) or SSH
/// (`git@github.com:<owner>/<repo>[.git]`), github.com only, schemes
/// lowercase-only, exactly like the regexes.
fn parse_github_repo_url(url: &str) -> Option<(String, String)> {
    if url.is_empty() {
        return None;
    }
    let candidate = url.trim();
    if candidate.is_empty() {
        return None;
    }
    if let Some(https) = candidate
        .strip_prefix("https://")
        .or_else(|| candidate.strip_prefix("http://"))
    {
        let after_host = https.strip_prefix("github.com")?;
        let rest = after_host.strip_prefix('/')?;
        return split_owner_name(rest, true);
    }
    if let Some(rest) = candidate.strip_prefix("git@github.com:") {
        return split_owner_name(rest, false);
    }
    None
}

/// `parse_github_pull_request_url` (`github_client.py:259-275`):
/// `https?://github.com/<owner>/<repo>/pull/<n>[/...]`. The repo name
/// keeps a `.git` suffix here (no suffix group in the regex); the number
/// renders canonically (`str(int(n))`, leading zeros stripped).
fn parse_github_pull_request_url(url: &str) -> Option<(String, String, String)> {
    if url.is_empty() {
        return None;
    }
    let candidate = url.trim();
    let https = candidate
        .strip_prefix("https://")
        .or_else(|| candidate.strip_prefix("http://"))?;
    let after_host = https.strip_prefix("github.com")?;
    let rest = after_host.strip_prefix('/')?;
    let mut segments = rest.split('/');
    let owner = segments.next()?;
    let name = segments.next()?;
    if segments.next() != Some("pull") {
        return None;
    }
    let number = segments.next()?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if owner.is_empty()
        || name.is_empty()
        || has_whitespace(owner)
        || has_whitespace(name)
        || has_whitespace(number)
    {
        return None;
    }
    for tail in segments {
        if has_whitespace(tail) {
            return None;
        }
    }
    let canonical = number.trim_start_matches('0');
    let canonical = if canonical.is_empty() { "0" } else { canonical };
    Some((
        owner.to_lowercase(),
        name.to_lowercase(),
        canonical.to_owned(),
    ))
}

/// `int(s)` for JSON string ids (`github.py:197,221,233`).
///
/// Python accepts surrounding whitespace and one leading sign with ASCII
/// digits (underscores and non-ASCII digits noted in the module docs).
/// Failure spells Python's message: `invalid literal for int() with base
/// 10: '…'`; overflow (Python ints are unbounded) is the same error.
fn py_int_from_str(raw: &str) -> Result<i64, GitProviderError> {
    let trimmed = raw.trim();
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(parsed) = trimmed.parse::<i64>() {
            return Ok(parsed);
        }
    }
    Err(GitProviderError::General(format!(
        "invalid literal for int() with base 10: '{raw}'"
    )))
}

/// `int(credential["installation_id"])` for a JSON value (`github.py:93`):
/// numbers truncate toward zero like Python's `int()`; strings parse via
/// [`py_int_from_str`]; `True`/`False` are `1`/`0` (bool is an int
/// subclass); anything else raises (`TypeError`/`ValueError` → passthrough).
fn py_int_from_json(value: &Value) -> Result<i64, GitProviderError> {
    match value {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i)
            } else if let Some(u) = n.as_u64() {
                i64::try_from(u)
                    .map_err(|_| GitProviderError::General(format!("int() overflow: {n}")))
            } else if let Some(f) = n.as_f64() {
                if f.is_finite() && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
                    Ok(f.trunc() as i64)
                } else {
                    Err(GitProviderError::General(format!("int() overflow: {n}")))
                }
            } else {
                Err(GitProviderError::General(format!("int() overflow: {n}")))
            }
        }
        Value::String(s) => py_int_from_str(s),
        Value::Bool(true) => Ok(1),
        Value::Bool(false) => Ok(0),
        other => Err(GitProviderError::General(format!(
            "int() argument must be a string or a number, not '{}'",
            json_type_name(other)
        ))),
    }
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

impl<C: GithubClient> GitHubAdapter<C> {
    /// `_token` (`github.py:87-91`): the stored PAT decrypted, falling back
    /// to the raw value when decryption fails closed. An empty token raises
    /// `GitProviderError::Auth("GitHub token is missing")`.
    ///
    /// `Keyring::decrypt` never fails outwardly (it yields `""`, mirroring
    /// `decrypt_data`'s log-and-empty wart), so a non-empty input
    /// decrypting to `""` *is* the failure signal — and the golden's
    /// fallback branch. A stored plaintext token therefore also passes
    /// through verbatim.
    fn token(&self, credential: &Value) -> Result<String, GitProviderError> {
        let raw = credential
            .get("token")
            .and_then(Value::as_str)
            .unwrap_or("");
        if raw.is_empty() {
            return Err(GitProviderError::Auth("GitHub token is missing".to_owned()));
        }
        let decrypted = self.keyring.decrypt(raw);
        if decrypted.is_empty() {
            Ok(raw.to_owned())
        } else {
            Ok(decrypted)
        }
    }

    /// `_client` (`github.py:92-96`): GitHub App credentials (with a truthy
    /// `installation_id`) connect for the installation; anything else
    /// connects with the PAT. Transport failures map via
    /// [`map_github_error`]; the missing-token `Auth` passes through as-is,
    /// exactly like Python (raised outside `_map_error`'s reach).
    fn client(&self, credential: &Value) -> Result<C, GitProviderError> {
        let auth_type = credential
            .get("auth_type")
            .and_then(Value::as_str)
            .unwrap_or("");
        if auth_type == "github_app" {
            if let Some(raw_id) = credential.get("installation_id") {
                if json_truthy(raw_id) {
                    let installation_id = py_int_from_json(raw_id)?;
                    return C::connect(&ClientAuth::Installation(installation_id))
                        .map_err(map_github_error);
                }
            }
        }
        let token = self.token(credential)?;
        C::connect(&ClientAuth::Token(token)).map_err(map_github_error)
    }

    /// `_remote_repo` (`github.py:126-144`).
    ///
    /// The `owner` dance mirrors `(payload.get("owner") or {}).get("login")
    /// or payload.get("owner") or ""` exactly, including its crash paths
    /// (see the module ported-bugs note): a truthy non-mapping owner, or a
    /// mapping owner with no usable string login, is a `General` error.
    fn remote_repo(payload: &Value) -> Result<RemoteRepository, GitProviderError> {
        let owner_value = payload.get("owner");
        let login_value: Option<&Value> = match owner_value {
            None => None,
            Some(value) if !json_truthy(value) => None,
            Some(Value::Object(map)) => map.get("login"),
            Some(Value::String(_)) => {
                return Err(GitProviderError::General(
                    "'str' object has no attribute 'get'".to_owned(),
                ));
            }
            Some(other) => {
                return Err(GitProviderError::General(format!(
                    "'{}' object has no attribute 'get'",
                    json_type_name(other).to_lowercase()
                )));
            }
        };
        let owner = match login_value {
            Some(Value::String(text)) if !text.is_empty() => text.to_owned(),
            // A truthy non-string login survives the `or` chain itself and
            // then fails `.lower` on its own type (`login=5` → `'int' …`).
            Some(value) if json_truthy(value) => {
                return Err(GitProviderError::General(format!(
                    "'{}' object has no attribute 'lower'",
                    json_type_name(value).to_lowercase()
                )));
            }
            _ => match owner_value {
                // Reached only when the login is absent/falsy: a truthy
                // owner here is necessarily a mapping (truthy strings and
                // other scalars errored above), and mappings have no
                // `.lower` — the second ported crash path.
                Some(value) if json_truthy(value) => {
                    return Err(GitProviderError::General(
                        "'dict' object has no attribute 'lower'".to_owned(),
                    ));
                }
                _ => String::new(),
            },
        };
        let owner = owner.to_lowercase();
        let name = match payload.get("name") {
            None => String::new(),
            Some(Value::String(text)) => text.to_lowercase(),
            Some(value) if !json_truthy(value) => String::new(),
            Some(other) => {
                return Err(GitProviderError::General(format!(
                    "'{}' object has no attribute 'lower'",
                    json_type_name(other).to_lowercase()
                )));
            }
        };
        let full_name = match payload.get("full_name") {
            Some(Value::String(text)) if !text.is_empty() => text.to_lowercase(),
            Some(value) if json_truthy(value) => {
                return Err(GitProviderError::General(format!(
                    "'{}' object has no attribute 'lower'",
                    json_type_name(value).to_lowercase()
                )));
            }
            _ => format!("{owner}/{name}").to_lowercase(),
        };
        let namespace = match full_name.rsplit_once('/') {
            Some((head, _)) => head.to_owned(),
            None => owner.clone(),
        };
        Ok(RemoteRepository {
            provider: GitHubAdapter::<C>::KEY.to_owned(),
            external_id: py_id_or_empty(payload.get("id")),
            namespace,
            name,
            full_name: full_name.clone(),
            web_url: match payload.get("html_url") {
                Some(Value::String(text)) if !text.is_empty() => text.clone(),
                _ => format!("{GITHUB_HOST}/{full_name}"),
            },
            clone_url_http: py_str_or_empty(payload.get("clone_url")),
            clone_url_ssh: py_str_or_empty(payload.get("ssh_url")),
            default_branch: py_str_or_empty(payload.get("default_branch")),
            is_private: json_truthy_opt(payload.get("private")),
            metadata: payload.clone(),
        })
    }

    /// `owner, name = repository.full_name.split("/", 1)`
    /// (`github.py:163,191,214`): the split sits *outside* the Python
    /// `try`, so a missing slash propagates unmapped. The message spells
    /// CPython's `ValueError`.
    fn split_full_name(full_name: &str) -> Result<(&str, &str), GitProviderError> {
        full_name.split_once('/').ok_or_else(|| {
            GitProviderError::General("not enough values to unpack (expected 2, got 1)".to_owned())
        })
    }

    /// One `RemoteIssue` row (`github.py:166-182`): PRs are skipped by
    /// `pull_request` key presence; `user` follows the same
    /// mapping-or-crash rule as a repo `owner`.
    fn remote_issue(issue: &Value) -> Result<Option<RemoteIssue>, GitProviderError> {
        let body = issue.as_object().ok_or_else(|| {
            GitProviderError::General(format!(
                "argument of type '{}' is not iterable",
                json_type_name(issue).to_lowercase()
            ))
        })?;
        if body.contains_key("pull_request") {
            return Ok(None);
        }
        let author = match issue.get("user") {
            None => String::new(),
            Some(value) if !json_truthy(value) => String::new(),
            Some(Value::Object(_)) => {
                py_str_or_empty(issue.get("user").and_then(|u| u.get("login")))
            }
            Some(Value::String(_)) => {
                return Err(GitProviderError::General(
                    "'str' object has no attribute 'get'".to_owned(),
                ));
            }
            Some(other) => {
                return Err(GitProviderError::General(format!(
                    "'{}' object has no attribute 'get'",
                    json_type_name(other).to_lowercase()
                )));
            }
        };
        Ok(Some(RemoteIssue {
            external_id: py_id_or_empty(issue.get("id")),
            external_iid: py_id_or_empty(issue.get("number")),
            title: py_str_or_empty(issue.get("title")),
            body: py_str_or_empty(issue.get("body")),
            state: py_str_or_empty(issue.get("state")),
            author,
            web_url: py_str_or_empty(issue.get("html_url")),
            created_at: parse_dt(issue.get("created_at").and_then(Value::as_str)),
            updated_at: parse_dt(issue.get("updated_at").and_then(Value::as_str)),
            metadata: issue.clone(),
        }))
    }

    /// One `RemoteComment` row (`github.py:193-205,222-229`).
    fn remote_comment(comment: &Value) -> Result<RemoteComment, GitProviderError> {
        if comment.as_object().is_none() {
            return Err(GitProviderError::General(format!(
                "argument of type '{}' is not iterable",
                json_type_name(comment).to_lowercase()
            )));
        }
        let author = match comment.get("user") {
            None => String::new(),
            Some(value) if !json_truthy(value) => String::new(),
            Some(Value::Object(_)) => {
                py_str_or_empty(comment.get("user").and_then(|u| u.get("login")))
            }
            Some(Value::String(_)) => {
                return Err(GitProviderError::General(
                    "'str' object has no attribute 'get'".to_owned(),
                ));
            }
            Some(other) => {
                return Err(GitProviderError::General(format!(
                    "'{}' object has no attribute 'get'",
                    json_type_name(other).to_lowercase()
                )));
            }
        };
        Ok(RemoteComment {
            external_id: py_id_or_empty(comment.get("id")),
            body: py_str_or_empty(comment.get("body")),
            author,
            web_url: py_str_or_empty(comment.get("html_url")),
            created_at: parse_dt(comment.get("created_at").and_then(Value::as_str)),
            updated_at: parse_dt(comment.get("updated_at").and_then(Value::as_str)),
            metadata: comment.clone(),
        })
    }
}

/// Display-only PR snapshot (`pr_snapshot_from_payload`,
/// `utils/github_client.py:287-300`).
///
/// `merged` derives from either `merged` or `merged_at` (webhooks report
/// the latter); `title` is truncated to 500 *characters* (Python slicing
/// counts code points, safe on UTF-8 boundaries); any non-`"closed"`
/// state — including a missing one — becomes `"open"`.
struct PrSnapshot {
    title: String,
    state: String,
    merged: bool,
    draft: bool,
    pr_updated_at: Option<String>,
}

fn pr_snapshot(pull_request: &Value) -> PrSnapshot {
    let merged = json_truthy_opt(pull_request.get("merged"))
        || json_truthy_opt(pull_request.get("merged_at"));
    let title: String = py_str_or_empty(pull_request.get("title"))
        .chars()
        .take(500)
        .collect();
    let state = match pull_request.get("state") {
        Some(Value::String(text)) if text == "closed" => "closed".to_owned(),
        _ => "open".to_owned(),
    };
    PrSnapshot {
        title,
        state,
        merged,
        draft: json_truthy_opt(pull_request.get("draft")),
        pr_updated_at: parse_dt(pull_request.get("updated_at").and_then(Value::as_str)),
    }
}

impl<C: GithubClient> GitProviderAdapter for GitHubAdapter<C> {
    const KEY: &'static str = "github";
    const DISPLAY_NAME: &'static str = "GitHub";
    const CODE_REVIEW_TERM: &'static str = "pull request";

    /// `parse_repo_url` (`github.py:55-70`).
    fn parse_repo_url(&self, url: &str) -> Option<ParsedRepository> {
        let (owner, name) = parse_github_repo_url(url)?;
        Some(ParsedRepository {
            provider: Self::KEY.to_owned(),
            host_url: GITHUB_HOST.to_owned(),
            namespace: owner.clone(),
            name: name.clone(),
            full_name: format!("{owner}/{name}"),
            clone_url: url.trim().to_owned(),
        })
    }

    /// `parse_code_review_url` (`github.py:71-86`).
    fn parse_code_review_url(&self, url: &str) -> Option<ParsedCodeReview> {
        let (owner, name, number) = parse_github_pull_request_url(url)?;
        Some(ParsedCodeReview {
            provider: Self::KEY.to_owned(),
            host_url: GITHUB_HOST.to_owned(),
            namespace: owner.clone(),
            repo_name: name.clone(),
            external_iid: number.clone(),
            url: format!("{GITHUB_HOST}/{owner}/{name}/pull/{number}"),
        })
    }

    /// `verify_provider_account` (`github.py:110-115`): `GET /user`,
    /// errors mapped.
    fn verify_provider_account(&self, credential: &Value) -> Result<Value, GitProviderError> {
        let client = self.client(credential)?;
        client.get_authenticated_user().map_err(map_github_error)
    }

    /// `credential_capabilities` (`github.py:116-125`): `auth_type or
    /// "pat"`; PATs write comments, GitHub Apps manage webhooks, nobody
    /// clones.
    fn credential_capabilities(
        &self,
        credential: &Value,
    ) -> Result<GitProviderCapabilities, GitProviderError> {
        // `auth_type = credential.get("auth_type") or "pat"`: only a
        // non-empty string overrides the default; a truthy non-string
        // stays itself and matches neither comparison below.
        let raw = credential.get("auth_type");
        let auth_type: Option<&str> = match raw {
            Some(Value::String(text)) if !text.is_empty() => Some(text.as_str()),
            Some(value) if json_truthy(value) => None,
            _ => Some("pat"),
        };
        Ok(GitProviderCapabilities {
            read_repositories: true,
            read_issues: true,
            write_comments: auth_type == Some("pat"),
            manage_webhooks: auth_type == Some("github_app"),
            clone: false,
        })
    }

    /// `list_repositories` (`github.py:145-155`): one
    /// `list_user_repos(page=page)` call; the `page` default (`1`) is
    /// passed explicitly — Rust has no default arguments.
    fn list_repositories(
        &self,
        credential: &Value,
        page: i64,
    ) -> Result<RepositoryPage, GitProviderError> {
        let client = self.client(credential)?;
        let (repos, has_next_page) = client.list_user_repos(page).map_err(map_github_error)?;
        let repositories = repos
            .iter()
            .map(Self::remote_repo)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(RepositoryPage {
            repositories,
            page,
            has_next_page,
        })
    }

    /// `get_repository` (`github.py:156-161`).
    fn get_repository(
        &self,
        credential: &Value,
        parsed: &ParsedRepository,
    ) -> Result<RemoteRepository, GitProviderError> {
        let client = self.client(credential)?;
        let payload = client
            .get_repo(&parsed.namespace, &parsed.name)
            .map_err(map_github_error)?;
        Self::remote_repo(&payload)
    }

    /// `list_open_issues` (`github.py:162-184`): issues only (PRs skipped),
    /// collected to a `Vec` — Python yields, but every item is consumed
    /// under the same `try`, so collection is equivalent.
    fn list_open_issues(
        &self,
        credential: &Value,
        repository: &RemoteRepository,
    ) -> Result<Vec<RemoteIssue>, GitProviderError> {
        let (owner, name) = Self::split_full_name(&repository.full_name)?;
        let client = self.client(credential)?;
        let issues = client
            .list_all_open_issues(owner, name)
            .map_err(map_github_error)?;
        let mut rows = Vec::new();
        for issue in &issues {
            if let Some(row) = Self::remote_issue(issue)? {
                rows.push(row);
            }
        }
        Ok(rows)
    }

    /// `list_issue_comments` (`github.py:185-206`).
    fn list_issue_comments(
        &self,
        credential: &Value,
        repository: &RemoteRepository,
        issue_iid: &str,
    ) -> Result<Vec<RemoteComment>, GitProviderError> {
        let (owner, name) = Self::split_full_name(&repository.full_name)?;
        let client = self.client(credential)?;
        let issue_number = py_int_from_str(issue_iid)?;
        let comments = client
            .list_issue_comments(owner, name, issue_number)
            .map_err(map_github_error)?;
        comments.iter().map(Self::remote_comment).collect()
    }

    /// `post_issue_comment` (`github.py:207-229`).
    fn post_issue_comment(
        &self,
        credential: &Value,
        repository: &RemoteRepository,
        issue_iid: &str,
        body: &str,
    ) -> Result<RemoteComment, GitProviderError> {
        let (owner, name) = Self::split_full_name(&repository.full_name)?;
        let client = self.client(credential)?;
        let issue_number = py_int_from_str(issue_iid)?;
        let comment = client
            .post_issue_comment(owner, name, issue_number, body)
            .map_err(map_github_error)?;
        Self::remote_comment(&comment)
    }

    /// `get_code_review` (`github.py:230-248`): `state` is `"merged"` when
    /// the snapshot says so, else the snapshot state; `external_iid` falls
    /// back to the parsed one when the payload number is falsy; `title`
    /// prefers the (truncated) snapshot title; `web_url` falls back to the
    /// parsed URL.
    fn get_code_review(
        &self,
        credential: &Value,
        parsed: &ParsedCodeReview,
    ) -> Result<RemoteCodeReview, GitProviderError> {
        let client = self.client(credential)?;
        let number = py_int_from_str(&parsed.external_iid)?;
        let pr = client
            .get_pull_request(&parsed.namespace, &parsed.repo_name, number)
            .map_err(map_github_error)?;
        let snapshot = pr_snapshot(&pr);
        let state = if snapshot.merged {
            "merged".to_owned()
        } else {
            snapshot.state.clone()
        };
        let external_iid = if json_truthy_opt(pr.get("number")) {
            py_id_or_empty(pr.get("number"))
        } else {
            parsed.external_iid.clone()
        };
        let title = if snapshot.title.is_empty() {
            py_str_or_empty(pr.get("title"))
        } else {
            snapshot.title.clone()
        };
        let web_url = match pr.get("html_url") {
            Some(Value::String(text)) if !text.is_empty() => text.clone(),
            _ => parsed.url.clone(),
        };
        Ok(RemoteCodeReview {
            external_id: py_id_or_empty(pr.get("id")),
            external_iid,
            title,
            state,
            merged: snapshot.merged,
            draft: snapshot.draft,
            web_url,
            updated_at: snapshot.pr_updated_at.clone(),
            metadata: pr,
        })
    }

    /// `normalize_webhook` (`github.py:249-257`): empty bodies read as
    /// `{}`; the event comes from `X-GitHub-Event` (either case) and the
    /// action from the payload. Refs keep their `pidash-types` defaults.
    fn normalize_webhook(
        &self,
        raw_body: &[u8],
        headers: &Value,
    ) -> Result<ProviderWebhookEvent, GitProviderError> {
        let text = std::str::from_utf8(raw_body)
            .map_err(|err| GitProviderError::General(format!("utf-8 decode failed: {err}")))?;
        let payload: Value = if text.is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str(text)
                .map_err(|err| GitProviderError::General(format!("invalid json: {err}")))?
        };
        let event = headers
            .get("X-GitHub-Event")
            .or_else(|| headers.get("x-github-event"))
            .and_then(Value::as_str)
            .unwrap_or("");
        Ok(ProviderWebhookEvent {
            provider: Self::KEY.to_owned(),
            event: event.to_owned(),
            action: py_str_or_empty(payload.get("action")),
            repository_ref: Value::Object(Default::default()),
            code_review_ref: Value::Object(Default::default()),
            issue_ref: Value::Object(Default::default()),
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `rust-api/fixtures/integrations/adapters/github.golden.json`.
    fn golden() -> Value {
        let path = format!(
            "{}/../../fixtures/integrations/adapters/github.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn test_adapter<C: GithubClient>() -> GitHubAdapter<C> {
        // A secret the tests never encrypt with, so stored plaintext
        // tokens exercise the raw-fallback branch.
        GitHubAdapter::new(Keyring::from_secret("test-secret-key"))
    }

    fn pat_credential() -> Value {
        json!({"token": "raw-pat", "auth_type": "pat"})
    }

    fn parsed_repo() -> ParsedRepository {
        ParsedRepository {
            provider: "github".into(),
            host_url: GITHUB_HOST.into(),
            namespace: "acme".into(),
            name: "web".into(),
            full_name: "acme/web".into(),
            clone_url: "https://github.com/acme/web".into(),
        }
    }

    fn remote_repo() -> RemoteRepository {
        RemoteRepository {
            provider: "github".into(),
            external_id: "1".into(),
            namespace: "acme".into(),
            name: "web".into(),
            full_name: "acme/web".into(),
            web_url: "https://github.com/acme/web".into(),
            clone_url_http: String::new(),
            clone_url_ssh: String::new(),
            default_branch: "main".into(),
            is_private: false,
            metadata: json!({}),
        }
    }

    fn repo_payload() -> Value {
        json!({
            "id": 111,
            "name": "Web",
            "full_name": "Acme/Web",
            "owner": {"login": "Acme"},
            "html_url": "https://github.com/acme/web",
            "clone_url": "https://github.com/acme/web.git",
            "ssh_url": "git@github.com:acme/web.git",
            "default_branch": "main",
            "private": false,
        })
    }

    // -- adapter keys (KEY / DISPLAY_NAME / CODE_REVIEW_TERM) --

    #[test]
    fn adapter_keys_match_golden() {
        let keys = &golden()["adapter_keys"];
        assert_eq!(
            GitHubAdapter::<NullClient>::KEY,
            keys["key"].as_str().unwrap()
        );
        assert_eq!(
            GitHubAdapter::<NullClient>::DISPLAY_NAME,
            keys["display_name"].as_str().unwrap()
        );
        assert_eq!(
            GitHubAdapter::<NullClient>::CODE_REVIEW_TERM,
            keys["code_review_term"].as_str().unwrap()
        );
    }

    /// Transport that must never be built; for credential-only paths.
    struct NullClient;
    impl GithubClient for NullClient {
        fn connect(_auth: &ClientAuth) -> Result<Self, GithubError> {
            unimplemented!("credential rejected before connect")
        }
        fn get_authenticated_user(&self) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn list_user_repos(&self, _page: i64) -> Result<(Vec<Value>, bool), GithubError> {
            unimplemented!()
        }
        fn get_repo(&self, _owner: &str, _name: &str) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn list_all_open_issues(
            &self,
            _owner: &str,
            _name: &str,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn list_issue_comments(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn post_issue_comment(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
            _body: &str,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn get_pull_request(
            &self,
            _owner: &str,
            _name: &str,
            _number: i64,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
    }

    // -- _parse_dt --

    #[test]
    fn parse_dt_replays_golden() {
        let dt = &golden()["_parse_dt"];
        assert_eq!(
            parse_dt(Some("2024-01-02T03:04:05+00:00")).as_deref(),
            dt["offset"].as_str()
        );
        assert_eq!(
            parse_dt(Some("2024-01-02T03:04:05Z")).as_deref(),
            dt["tz_zulu"].as_str()
        );
        assert_eq!(parse_dt(Some("")), None);
        assert!(dt["empty_string"].is_null());
        assert_eq!(parse_dt(Some("not-a-date")), None);
        assert!(dt["garbage"].is_null());
        assert_eq!(parse_dt(None), None);
    }

    #[test]
    fn parse_dt_renders_python_isoformat_shapes() {
        assert_eq!(
            parse_dt(Some("2024-06-01T12:00:00+05:30")).as_deref(),
            Some("2024-06-01T12:00:00+05:30")
        );
        assert_eq!(
            parse_dt(Some("2024-01-02T03:04:05.123456Z")).as_deref(),
            Some("2024-01-02T03:04:05.123456+00:00")
        );
        // Zero fraction omitted, like isoformat().
        assert_eq!(
            parse_dt(Some("2024-01-02T03:04:05.000000Z")).as_deref(),
            Some("2024-01-02T03:04:05+00:00")
        );
        // Naive and date-only fallbacks render without offset.
        assert_eq!(
            parse_dt(Some("2024-01-02T03:04:05")).as_deref(),
            Some("2024-01-02T03:04:05")
        );
        assert_eq!(
            parse_dt(Some("2024-01-02")).as_deref(),
            Some("2024-01-02T00:00:00")
        );
        // Sub-microsecond precision truncates, like fromisoformat.
        assert_eq!(
            parse_dt(Some("2024-01-02T03:04:05.123456789Z")).as_deref(),
            Some("2024-01-02T03:04:05.123456+00:00")
        );
    }

    // -- parse_repo_url --

    #[test]
    fn parse_repo_url_replays_golden() {
        let adapter = test_adapter::<NullClient>();
        let gold = &golden()["parse_repo_url"];
        let parsed = adapter
            .parse_repo_url("https://github.com/Acme/Web.git")
            .expect("https .git parses");
        // Byte-exact struct serialization: field order is declaration order.
        assert_eq!(
            serde_json::to_string(&parsed).expect("serializes"),
            r#"{"provider":"github","host_url":"https://github.com","namespace":"acme","name":"web","full_name":"acme/web","clone_url":"https://github.com/Acme/Web.git"}"#
        );
        assert_eq!(parsed.clone_url, "https://github.com/Acme/Web.git");
        assert_eq!(
            parsed.full_name,
            gold["ok_https_git_suffix"]["full_name"].as_str().unwrap()
        );
        for (input, key) in [
            ("http://github.com/acme/web", "http_scheme_ok"),
            ("git@github.com:Acme/Web.git", "ok_ssh"),
        ] {
            let row = adapter.parse_repo_url(input).expect("parses");
            assert_eq!(
                row.full_name,
                gold[key]["full_name"].as_str().unwrap(),
                "{input}"
            );
        }
        assert!(adapter.parse_repo_url("").is_none());
        assert!(gold["empty"].is_null());
        assert!(adapter
            .parse_repo_url("https://gitlab.com/acme/web")
            .is_none());
        assert!(gold["gitlab_host"].is_null());
    }

    #[test]
    fn parse_repo_url_rejects_non_repo_shapes() {
        let adapter = test_adapter::<NullClient>();
        // Trailing slash is fine; double slashes, extra segments, issue
        // URLs, wrong hosts and uppercase schemes are not.
        assert!(adapter
            .parse_repo_url("https://github.com/acme/web/")
            .is_some());
        for bad in [
            "https://github.com/acme/web//",
            "https://github.com/acme/web/x",
            "https://github.com/acme/web/pull/42",
            "https://github.com/acme",
            "https://GITHUB.COM/acme/web",
            "HTTPS://github.com/acme/web",
            "git@github.com:acme/web/",
            "git@gitlab.com:acme/web.git",
            "https://github.com/acme/x.git.git/y",
        ] {
            assert!(adapter.parse_repo_url(bad).is_none(), "{bad}");
        }
        // Lazy `.git` rule: only one suffix strips, and only when a name
        // remains — `x.gitgit` keeps both.
        let kept = adapter
            .parse_repo_url("https://github.com/acme/x.gitgit")
            .expect("parses");
        assert_eq!(kept.name, "x.gitgit");
        let stripped = adapter
            .parse_repo_url("https://github.com/acme/x.git")
            .expect("parses");
        assert_eq!(stripped.name, "x");
        // Clone URL is the raw stripped input, never lowercased.
        let raw = adapter
            .parse_repo_url("  https://github.com/Acme/Web.git  ")
            .expect("parses");
        assert_eq!(raw.clone_url, "https://github.com/Acme/Web.git");
    }

    // -- parse_code_review_url --

    #[test]
    fn parse_code_review_url_replays_golden() {
        let adapter = test_adapter::<NullClient>();
        let gold = &golden()["parse_code_review_url"]["ok"];
        let parsed = adapter
            .parse_code_review_url("https://github.com/acme/web/pull/42")
            .expect("pr url parses");
        assert_eq!(
            serde_json::to_string(&parsed).expect("serializes"),
            r#"{"provider":"github","host_url":"https://github.com","namespace":"acme","repo_name":"web","external_iid":"42","url":"https://github.com/acme/web/pull/42"}"#
        );
        assert_eq!(parsed.external_iid, gold["external_iid"].as_str().unwrap());
        assert_eq!(parsed.url, gold["url"].as_str().unwrap());
        assert_eq!(parsed.namespace, gold["namespace"].as_str().unwrap());
        assert_eq!(parsed.repo_name, gold["repo_name"].as_str().unwrap());
        let trailing = adapter
            .parse_code_review_url("https://github.com/acme/web/pull/42/files")
            .expect("trailing path parses");
        assert_eq!(
            trailing.external_iid,
            golden()["parse_code_review_url"]["ok_trailing_path"]["external_iid"]
                .as_str()
                .unwrap()
        );
        assert_eq!(
            trailing.url,
            golden()["parse_code_review_url"]["ok_trailing_path"]["url"]
                .as_str()
                .unwrap()
        );
        for bad in [
            "https://github.com/acme/web/issues/42",
            "https://gitlab.com/acme/web/pull/42",
            "https://github.com/acme/web/pull/abc",
        ] {
            assert!(adapter.parse_code_review_url(bad).is_none(), "{bad}");
        }
        let gold_nulls = &golden()["parse_code_review_url"];
        assert!(gold_nulls["issues_url"].is_null());
        assert!(gold_nulls["non_github_host"].is_null());
        assert!(gold_nulls["non_numeric"].is_null());
    }

    #[test]
    fn parse_code_review_url_canonicalizes_number() {
        let adapter = test_adapter::<NullClient>();
        // str(int(n)): leading zeros strip; the rebuilt URL is canonical.
        let parsed = adapter
            .parse_code_review_url("https://github.com/Acme/Web/pull/007")
            .expect("parses");
        assert_eq!(parsed.external_iid, "7");
        assert_eq!(parsed.url, "https://github.com/acme/web/pull/7");
        assert!(adapter
            .parse_code_review_url("https://github.com/acme/web/pull/")
            .is_none());
    }

    // -- _map_error --

    #[test]
    fn error_mapping_matches_golden_cases() {
        let gold = &golden()["error_mapping"];
        assert_eq!(
            map_github_error(GithubError::Auth("bad".into())),
            GitProviderError::Auth("bad".into())
        );
        assert_eq!(
            gold["GithubAuthError"].as_str().unwrap(),
            "GitProviderAuthError"
        );
        assert_eq!(
            map_github_error(GithubError::Permission("no".into())),
            GitProviderError::Permission("no".into())
        );
        assert_eq!(
            gold["GithubPermissionError"].as_str().unwrap(),
            "GitProviderPermissionError"
        );
        assert_eq!(
            map_github_error(GithubError::NotFound("gone".into())),
            GitProviderError::NotFound("gone".into())
        );
        assert_eq!(
            gold["GithubNotFoundError"].as_str().unwrap(),
            "GitProviderNotFoundError"
        );
        // Anything else passes through as the base error, message intact.
        assert_eq!(
            map_github_error(GithubError::Transport("boom".into())),
            GitProviderError::General("boom".into())
        );
        assert_eq!(
            gold["other_passthrough"].as_str().unwrap(),
            "returned unchanged"
        );
    }

    /// Transport failing with 401 surfaces as Auth end to end.
    struct UnauthorizedClient;
    impl GithubClient for UnauthorizedClient {
        fn connect(auth: &ClientAuth) -> Result<Self, GithubError> {
            assert!(matches!(auth, ClientAuth::Token(_)));
            Ok(UnauthorizedClient)
        }
        fn get_authenticated_user(&self) -> Result<Value, GithubError> {
            Err(GithubError::Auth("requires authentication".into()))
        }
        fn list_user_repos(&self, _page: i64) -> Result<(Vec<Value>, bool), GithubError> {
            unimplemented!()
        }
        fn get_repo(&self, _owner: &str, _name: &str) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn list_all_open_issues(
            &self,
            _owner: &str,
            _name: &str,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn list_issue_comments(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn post_issue_comment(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
            _body: &str,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn get_pull_request(
            &self,
            _owner: &str,
            _name: &str,
            _number: i64,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
    }

    #[test]
    fn transport_auth_error_maps_through_verify() {
        let adapter = test_adapter::<UnauthorizedClient>();
        assert_eq!(
            adapter.verify_provider_account(&pat_credential()),
            Err(GitProviderError::Auth("requires authentication".into()))
        );
    }

    // -- _token / _client --

    #[test]
    fn token_missing_raises_auth_matching_golden() {
        let adapter = test_adapter::<NullClient>();
        for credential in [json!({}), json!({"token": ""}), json!({"auth_type": "pat"})] {
            assert_eq!(
                adapter.verify_provider_account(&credential),
                Err(GitProviderError::Auth("GitHub token is missing".into())),
                "{credential}"
            );
        }
        assert_eq!(
            golden()["token_missing"]["error"].as_str().unwrap(),
            "GitProviderAuthError: GitHub token is missing"
        );
    }

    /// Transport asserting the exact auth it was built with.
    struct PatClient;
    impl GithubClient for PatClient {
        fn connect(auth: &ClientAuth) -> Result<Self, GithubError> {
            assert!(matches!(auth, ClientAuth::Token(token) if token == "raw-pat"));
            Ok(PatClient)
        }
        fn get_authenticated_user(&self) -> Result<Value, GithubError> {
            Ok(json!({"login": "octocat"}))
        }
        fn list_user_repos(&self, _page: i64) -> Result<(Vec<Value>, bool), GithubError> {
            unimplemented!()
        }
        fn get_repo(&self, _owner: &str, _name: &str) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn list_all_open_issues(
            &self,
            _owner: &str,
            _name: &str,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn list_issue_comments(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn post_issue_comment(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
            _body: &str,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn get_pull_request(
            &self,
            _owner: &str,
            _name: &str,
            _number: i64,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
    }

    #[test]
    fn undecryptable_token_falls_back_to_raw() {
        // "test-secret-key" never encrypted "raw-pat": decrypt fails
        // closed, so the client receives the raw value (golden source
        // note on adapters/github.py:87-94).
        let adapter = test_adapter::<PatClient>();
        assert_eq!(
            adapter.verify_provider_account(&pat_credential()),
            Ok(json!({"login": "octocat"}))
        );
    }

    #[test]
    fn encrypted_token_decrypts_before_connect() {
        let keyring = Keyring::from_secret("s3cret");
        let stored = keyring.encrypt("live-pat");
        let adapter = GitHubAdapter::<EncryptedClient>::new(keyring);
        assert_eq!(
            adapter.verify_provider_account(&json!({"token": stored})),
            Ok(json!({"login": "octocat"}))
        );
    }

    /// Transport asserting it received the decrypted PAT.
    struct EncryptedClient;
    impl GithubClient for EncryptedClient {
        fn connect(auth: &ClientAuth) -> Result<Self, GithubError> {
            assert!(matches!(auth, ClientAuth::Token(token) if token == "live-pat"));
            Ok(EncryptedClient)
        }
        fn get_authenticated_user(&self) -> Result<Value, GithubError> {
            Ok(json!({"login": "octocat"}))
        }
        fn list_user_repos(&self, _page: i64) -> Result<(Vec<Value>, bool), GithubError> {
            unimplemented!()
        }
        fn get_repo(&self, _owner: &str, _name: &str) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn list_all_open_issues(
            &self,
            _owner: &str,
            _name: &str,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn list_issue_comments(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn post_issue_comment(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
            _body: &str,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn get_pull_request(
            &self,
            _owner: &str,
            _name: &str,
            _number: i64,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
    }

    /// Transport asserting installation auth.
    struct InstallationClient;
    impl GithubClient for InstallationClient {
        fn connect(auth: &ClientAuth) -> Result<Self, GithubError> {
            assert!(matches!(auth, ClientAuth::Installation(7)));
            Ok(InstallationClient)
        }
        fn get_authenticated_user(&self) -> Result<Value, GithubError> {
            Ok(json!({"login": "app-bot"}))
        }
        fn list_user_repos(&self, _page: i64) -> Result<(Vec<Value>, bool), GithubError> {
            unimplemented!()
        }
        fn get_repo(&self, _owner: &str, _name: &str) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn list_all_open_issues(
            &self,
            _owner: &str,
            _name: &str,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn list_issue_comments(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn post_issue_comment(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
            _body: &str,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn get_pull_request(
            &self,
            _owner: &str,
            _name: &str,
            _number: i64,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
    }

    #[test]
    fn github_app_credential_connects_for_installation() {
        let adapter = test_adapter::<InstallationClient>();
        assert_eq!(
            adapter.verify_provider_account(
                &json!({"auth_type": "github_app", "installation_id": 7, "token": "unused"})
            ),
            Ok(json!({"login": "app-bot"}))
        );
    }

    #[test]
    fn github_app_without_installation_id_uses_pat() {
        // Falsy installation_id falls through to the PAT branch, which
        // then fails closed on the missing token.
        let adapter = test_adapter::<NullClient>();
        assert_eq!(
            adapter
                .verify_provider_account(&json!({"auth_type": "github_app", "installation_id": 0})),
            Err(GitProviderError::Auth("GitHub token is missing".into()))
        );
    }

    // -- credential_capabilities --

    #[test]
    fn capabilities_replay_golden() {
        let adapter = test_adapter::<NullClient>();
        let gold = &golden()["capability_map"];
        for (credential, key) in [
            (json!({"auth_type": "pat"}), "pat"),
            (json!({"auth_type": "github_app"}), "github_app"),
            (json!({}), "missing_auth_type_defaults_to_pat"),
        ] {
            let caps = adapter
                .credential_capabilities(&credential)
                .expect("infallible");
            assert_eq!(
                caps.read_repositories,
                gold[key]["read_repositories"].as_bool().unwrap(),
                "{key}"
            );
            assert_eq!(
                caps.read_issues,
                gold[key]["read_issues"].as_bool().unwrap(),
                "{key}"
            );
            assert_eq!(
                caps.write_comments,
                gold[key]["write_comments"].as_bool().unwrap(),
                "{key}"
            );
            assert_eq!(
                caps.manage_webhooks,
                gold[key]["manage_webhooks"].as_bool().unwrap(),
                "{key}"
            );
            assert_eq!(caps.clone, gold[key]["clone"].as_bool().unwrap(), "{key}");
        }
    }

    // -- list_repositories / get_repository --

    /// Transport serving one canned repo page.
    struct RepoPageClient;
    impl GithubClient for RepoPageClient {
        fn connect(auth: &ClientAuth) -> Result<Self, GithubError> {
            assert!(matches!(auth, ClientAuth::Token(token) if token == "raw-pat"));
            Ok(RepoPageClient)
        }
        fn get_authenticated_user(&self) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn list_user_repos(&self, page: i64) -> Result<(Vec<Value>, bool), GithubError> {
            assert_eq!(page, 2);
            Ok((
                vec![
                    repo_payload(),
                    json!({
                        "id": 0,
                        "owner": {},
                        "private": true,
                    }),
                ],
                true,
            ))
        }
        fn get_repo(&self, owner: &str, name: &str) -> Result<Value, GithubError> {
            assert_eq!((owner, name), ("acme", "web"));
            Ok(repo_payload())
        }
        fn list_all_open_issues(
            &self,
            _owner: &str,
            _name: &str,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn list_issue_comments(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn post_issue_comment(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
            _body: &str,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn get_pull_request(
            &self,
            _owner: &str,
            _name: &str,
            _number: i64,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
    }

    #[test]
    fn list_repositories_shapes_page_and_remote_repos() {
        let adapter = test_adapter::<RepoPageClient>();
        let page = adapter
            .list_repositories(&pat_credential(), 2)
            .expect("page");
        assert_eq!(page.page, 2);
        assert!(page.has_next_page);
        assert_eq!(page.repositories.len(), 2);
        let repo = &page.repositories[0];
        assert_eq!(repo.provider, "github");
        assert_eq!(repo.external_id, "111");
        assert_eq!(repo.namespace, "acme");
        assert_eq!(repo.name, "web");
        assert_eq!(repo.full_name, "acme/web");
        assert_eq!(repo.web_url, "https://github.com/acme/web");
        assert_eq!(repo.clone_url_http, "https://github.com/acme/web.git");
        assert_eq!(repo.clone_url_ssh, "git@github.com:acme/web.git");
        assert_eq!(repo.default_branch, "main");
        assert!(!repo.is_private);
        assert_eq!(repo.metadata, repo_payload());
        // Sparse payload: missing names fall back to owner/full_name
        // chains, ids render "", privacy is truthy.
        let sparse = &page.repositories[1];
        assert_eq!(sparse.external_id, "");
        assert_eq!(sparse.namespace, "");
        assert_eq!(sparse.full_name, "/");
        assert_eq!(sparse.web_url, "https://github.com//");
        assert!(sparse.is_private);
    }

    #[test]
    fn get_repository_delegates_to_client() {
        let adapter = test_adapter::<RepoPageClient>();
        let repo = adapter
            .get_repository(&pat_credential(), &parsed_repo())
            .expect("repo");
        assert_eq!(repo.full_name, "acme/web");
        assert_eq!(repo.external_id, "111");
    }

    #[test]
    fn remote_repo_ported_owner_crash_paths() {
        // Each expectation below was checked against the live Python
        // expression `(payload.get("owner") or {}).get("login") or
        // payload.get("owner") or ""` + `.lower()`: a string owner has no
        // `.get`; a truthy non-string login fails `.lower` on its own
        // type; a mapping owner without a usable login fails `.lower` on
        // the mapping. Falsy owners ({}, null, missing) degrade to "".
        for (payload, message) in [
            (
                json!({"owner": "acme"}),
                "'str' object has no attribute 'get'",
            ),
            (json!({"owner": 5}), "'int' object has no attribute 'get'"),
            (
                json!({"owner": {"login": null}}),
                "'dict' object has no attribute 'lower'",
            ),
            (
                json!({"owner": {"login": ""}}),
                "'dict' object has no attribute 'lower'",
            ),
            (
                json!({"owner": {"login": 5}}),
                "'int' object has no attribute 'lower'",
            ),
        ] {
            assert_eq!(
                GitHubAdapter::<NullClient>::remote_repo(&payload),
                Err(GitProviderError::General(message.into())),
                "{payload}"
            );
        }
        for payload in [json!({"owner": {}}), json!({"owner": null}), json!({})] {
            let repo = GitHubAdapter::<NullClient>::remote_repo(&payload).expect("falsy owner");
            assert_eq!(repo.namespace, "", "{payload}");
            assert_eq!(repo.full_name, "/", "{payload}");
        }
    }

    // -- list_open_issues --

    fn issue_payload(id: i64, number: i64) -> Value {
        json!({
            "id": id,
            "number": number,
            "title": "Bug",
            "body": "body text",
            "state": "open",
            "user": {"login": "alice"},
            "html_url": "https://github.com/acme/web/issues/1",
            "created_at": "2024-01-02T03:04:05Z",
            "updated_at": "2024-02-03T04:05:06+00:00",
        })
    }

    /// Transport serving two issues and one PR.
    struct IssuesClient;
    impl GithubClient for IssuesClient {
        fn connect(auth: &ClientAuth) -> Result<Self, GithubError> {
            assert!(matches!(auth, ClientAuth::Token(token) if token == "raw-pat"));
            Ok(IssuesClient)
        }
        fn get_authenticated_user(&self) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn list_user_repos(&self, _page: i64) -> Result<(Vec<Value>, bool), GithubError> {
            unimplemented!()
        }
        fn get_repo(&self, _owner: &str, _name: &str) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn list_all_open_issues(&self, owner: &str, name: &str) -> Result<Vec<Value>, GithubError> {
            assert_eq!((owner, name), ("acme", "web"));
            let mut pr = issue_payload(9, 3);
            pr["pull_request"] = json!({"url": "https://api.github.com/repos/acme/web/pulls/3"});
            // Key presence skips even when the value is null.
            let mut null_pr = issue_payload(10, 4);
            null_pr["pull_request"] = Value::Null;
            Ok(vec![issue_payload(1, 1), pr, null_pr, issue_payload(2, 2)])
        }
        fn list_issue_comments(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn post_issue_comment(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
            _body: &str,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn get_pull_request(
            &self,
            _owner: &str,
            _name: &str,
            _number: i64,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
    }

    #[test]
    fn list_open_issues_skips_pull_requests() {
        let adapter = test_adapter::<IssuesClient>();
        let rows = adapter
            .list_open_issues(&pat_credential(), &remote_repo())
            .expect("issues");
        assert_eq!(rows.len(), 2);
        let first = &rows[0];
        assert_eq!(first.external_id, "1");
        assert_eq!(first.external_iid, "1");
        assert_eq!(first.title, "Bug");
        assert_eq!(first.body, "body text");
        assert_eq!(first.state, "open");
        assert_eq!(first.author, "alice");
        assert_eq!(first.web_url, "https://github.com/acme/web/issues/1");
        assert_eq!(
            first.created_at.as_deref(),
            Some("2024-01-02T03:04:05+00:00")
        );
        assert_eq!(
            first.updated_at.as_deref(),
            Some("2024-02-03T04:05:06+00:00")
        );
        assert_eq!(first.metadata, issue_payload(1, 1));
        assert_eq!(rows[1].external_id, "2");
    }

    #[test]
    fn list_open_issues_without_slash_errors_unmapped() {
        let adapter = test_adapter::<IssuesClient>();
        let mut repo = remote_repo();
        repo.full_name = "noslash".into();
        assert_eq!(
            adapter.list_open_issues(&pat_credential(), &repo),
            Err(GitProviderError::General(
                "not enough values to unpack (expected 2, got 1)".into()
            ))
        );
    }

    // -- list_issue_comments / post_issue_comment --

    fn comment_payload() -> Value {
        json!({
            "id": 55,
            "body": "looks good",
            "user": {"login": "bob"},
            "html_url": "https://github.com/acme/web/issues/1#issuecomment-55",
            "created_at": "2024-03-04T05:06:07Z",
            "updated_at": null,
        })
    }

    /// Transport asserting comment call shapes.
    struct CommentsClient;
    impl GithubClient for CommentsClient {
        fn connect(auth: &ClientAuth) -> Result<Self, GithubError> {
            assert!(matches!(auth, ClientAuth::Token(token) if token == "raw-pat"));
            Ok(CommentsClient)
        }
        fn get_authenticated_user(&self) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn list_user_repos(&self, _page: i64) -> Result<(Vec<Value>, bool), GithubError> {
            unimplemented!()
        }
        fn get_repo(&self, _owner: &str, _name: &str) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn list_all_open_issues(
            &self,
            _owner: &str,
            _name: &str,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn list_issue_comments(
            &self,
            owner: &str,
            name: &str,
            issue_number: i64,
        ) -> Result<Vec<Value>, GithubError> {
            assert_eq!((owner, name, issue_number), ("acme", "web", 42));
            Ok(vec![comment_payload()])
        }
        fn post_issue_comment(
            &self,
            owner: &str,
            name: &str,
            issue_number: i64,
            body: &str,
        ) -> Result<Value, GithubError> {
            assert_eq!(
                (owner, name, issue_number, body),
                ("acme", "web", 42, "hello")
            );
            Ok(comment_payload())
        }
        fn get_pull_request(
            &self,
            _owner: &str,
            _name: &str,
            _number: i64,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
    }

    #[test]
    fn list_issue_comments_shapes_rows() {
        let adapter = test_adapter::<CommentsClient>();
        let rows = adapter
            .list_issue_comments(&pat_credential(), &remote_repo(), "42")
            .expect("comments");
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.external_id, "55");
        assert_eq!(row.body, "looks good");
        assert_eq!(row.author, "bob");
        assert_eq!(
            row.web_url,
            "https://github.com/acme/web/issues/1#issuecomment-55"
        );
        assert_eq!(row.created_at.as_deref(), Some("2024-03-04T05:06:07+00:00"));
        assert_eq!(row.updated_at, None);
    }

    #[test]
    fn comment_iid_parses_like_python_int() {
        // Whitespace and signs strip like int(); anything else spells
        // Python's literal error.
        assert_eq!(py_int_from_str("  +42 "), Ok(42));
        assert_eq!(py_int_from_str("042"), Ok(42));
        assert_eq!(
            py_int_from_str("4.5"),
            Err(GitProviderError::General(
                "invalid literal for int() with base 10: '4.5'".into()
            ))
        );
        let adapter = test_adapter::<CommentsClient>();
        assert!(adapter
            .list_issue_comments(&pat_credential(), &remote_repo(), "4x")
            .is_err());
    }

    #[test]
    fn post_issue_comment_returns_row() {
        let adapter = test_adapter::<CommentsClient>();
        let row = adapter
            .post_issue_comment(&pat_credential(), &remote_repo(), "42", "hello")
            .expect("comment");
        assert_eq!(row.external_id, "55");
        assert_eq!(row.body, "looks good");
        assert_eq!(row.author, "bob");
        assert_eq!(row.metadata, comment_payload());
    }

    // -- get_code_review --

    fn pr_payload() -> Value {
        json!({
            "id": 777,
            "number": 42,
            "title": "Add feature",
            "state": "closed",
            "merged": false,
            "merged_at": "2024-05-06T07:08:09Z",
            "draft": false,
            "html_url": "https://github.com/acme/web/pull/42",
            "updated_at": "2024-05-06T07:08:09Z",
        })
    }

    fn parsed_review() -> ParsedCodeReview {
        ParsedCodeReview {
            provider: "github".into(),
            host_url: GITHUB_HOST.into(),
            namespace: "acme".into(),
            repo_name: "web".into(),
            external_iid: "42".into(),
            url: "https://github.com/acme/web/pull/42".into(),
        }
    }

    /// Transport serving the merged PR.
    struct PullClient;
    impl GithubClient for PullClient {
        fn connect(auth: &ClientAuth) -> Result<Self, GithubError> {
            assert!(matches!(auth, ClientAuth::Token(token) if token == "raw-pat"));
            Ok(PullClient)
        }
        fn get_authenticated_user(&self) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn list_user_repos(&self, _page: i64) -> Result<(Vec<Value>, bool), GithubError> {
            unimplemented!()
        }
        fn get_repo(&self, _owner: &str, _name: &str) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn list_all_open_issues(
            &self,
            _owner: &str,
            _name: &str,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn list_issue_comments(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn post_issue_comment(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
            _body: &str,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn get_pull_request(
            &self,
            owner: &str,
            name: &str,
            number: i64,
        ) -> Result<Value, GithubError> {
            assert_eq!((owner, name, number), ("acme", "web", 42));
            Ok(pr_payload())
        }
    }

    #[test]
    fn get_code_review_marks_merged_from_merged_at() {
        // merged_at set (webhook shape) with merged=false still counts as
        // merged; state renders "merged" even though the PR is "closed".
        let adapter = test_adapter::<PullClient>();
        let review = adapter
            .get_code_review(&pat_credential(), &parsed_review())
            .expect("pr");
        assert_eq!(review.external_id, "777");
        assert_eq!(review.external_iid, "42");
        assert_eq!(review.title, "Add feature");
        assert_eq!(review.state, "merged");
        assert!(review.merged);
        assert!(!review.draft);
        assert_eq!(review.web_url, "https://github.com/acme/web/pull/42");
        assert_eq!(
            review.updated_at.as_deref(),
            Some("2024-05-06T07:08:09+00:00")
        );
        assert_eq!(review.metadata, pr_payload());
    }

    #[test]
    fn pr_snapshot_state_and_title_rules() {
        // Non-closed states (and missing ones) read "open".
        let open = pr_snapshot(&json!({"state": "open"}));
        assert_eq!(open.state, "open");
        assert!(!open.merged);
        let missing = pr_snapshot(&json!({}));
        assert_eq!(missing.state, "open");
        assert_eq!(missing.title, "");
        // Title truncates at 500 characters (code points, not bytes).
        let emoji = "é".repeat(600);
        let snap = pr_snapshot(&json!({"title": emoji}));
        assert_eq!(snap.title.chars().count(), 500);
        assert_eq!(snap.title, "é".repeat(500));
        // merged_at alone marks merged.
        let via_at = pr_snapshot(&json!({"merged_at": "2024-01-01T00:00:00Z"}));
        assert!(via_at.merged);
    }

    /// Transport serving a PR with missing number/title/url.
    struct SparsePullClient;
    impl GithubClient for SparsePullClient {
        fn connect(_auth: &ClientAuth) -> Result<Self, GithubError> {
            Ok(SparsePullClient)
        }
        fn get_authenticated_user(&self) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn list_user_repos(&self, _page: i64) -> Result<(Vec<Value>, bool), GithubError> {
            unimplemented!()
        }
        fn get_repo(&self, _owner: &str, _name: &str) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn list_all_open_issues(
            &self,
            _owner: &str,
            _name: &str,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn list_issue_comments(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
        ) -> Result<Vec<Value>, GithubError> {
            unimplemented!()
        }
        fn post_issue_comment(
            &self,
            _owner: &str,
            _name: &str,
            _issue_number: i64,
            _body: &str,
        ) -> Result<Value, GithubError> {
            unimplemented!()
        }
        fn get_pull_request(
            &self,
            _owner: &str,
            _name: &str,
            _number: i64,
        ) -> Result<Value, GithubError> {
            Ok(json!({"id": 1, "state": "open", "title": "T", "html_url": ""}))
        }
    }

    #[test]
    fn get_code_review_falls_back_to_parsed_refs() {
        let adapter = test_adapter::<SparsePullClient>();
        let review = adapter
            .get_code_review(&pat_credential(), &parsed_review())
            .expect("pr");
        assert_eq!(review.external_iid, "42");
        assert_eq!(review.title, "T");
        assert_eq!(review.state, "open");
        assert_eq!(review.web_url, "https://github.com/acme/web/pull/42");
    }

    // -- normalize_webhook --

    #[test]
    fn normalize_webhook_reads_headers_and_action() {
        let adapter = test_adapter::<NullClient>();
        let body = br#"{"action": "opened", "repository": {"id": 1}}"#;
        for headers in [
            json!({"X-GitHub-Event": "pull_request"}),
            json!({"x-github-event": "pull_request"}),
        ] {
            let event = adapter.normalize_webhook(body, &headers).expect("event");
            assert_eq!(event.provider, "github");
            assert_eq!(event.event, "pull_request");
            assert_eq!(event.action, "opened");
            assert_eq!(event.payload["repository"]["id"], json!(1));
            assert_eq!(event.repository_ref, json!({}));
            assert_eq!(event.code_review_ref, json!({}));
            assert_eq!(event.issue_ref, json!({}));
        }
        // Empty bodies read as {}; missing headers and actions read as "".
        let empty = adapter.normalize_webhook(b"", &json!({})).expect("empty");
        assert_eq!(empty.event, "");
        assert_eq!(empty.action, "");
        assert_eq!(empty.payload, json!({}));
        // Capital header wins over lowercase when both are present.
        let both = adapter
            .normalize_webhook(body, &json!({"X-GitHub-Event": "a", "x-github-event": "b"}))
            .expect("both");
        assert_eq!(both.event, "a");
        // Invalid JSON is a base (non-provider) error, like Python's
        // JSONDecodeError passing through _map_error untouched.
        assert!(matches!(
            adapter.normalize_webhook(b"{nope", &json!({})),
            Err(GitProviderError::General(_))
        ));
    }
}
