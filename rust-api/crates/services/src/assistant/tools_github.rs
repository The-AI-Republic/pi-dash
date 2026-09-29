//! Assistant git-provider read tool (D-06, stage 5).
//!
//! Pure port of `apps/api/pi_dash/assistant/tools/github.py:1-165`:
//! `get_pull_request_status` (`:85-165`), the token lookup
//! (`_find_token :48-82`), `_maybe_decrypt :39-45`, and `_unknown :35-36`.
//!
//! The tool **never raises** (`github.py:5-12`): every failure —
//! unsupported URL, budget exhaustion, SSRF block, transport error,
//! rate limit, unexpected status, bad JSON — is a normal
//! `{"state": "unknown", "reason": ...}` answer the loop's auto-close
//! prompt is written around. This module ports the decision tree as pure
//! functions; the handler layer performs the HTTP call (10s timeout, no
//! redirects) and the two token SQL lookups below.
//!
//! Fixture: `rust-api/fixtures/assistant/tools-tasks.json` (`F-A6-10`,
//! `tools.github`).

use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// `github.py:31` — GitHub REST API host.
pub const GITHUB_API_HOST: &str = "https://api.github.com";
/// `github.py:32` — per-request timeout, seconds.
pub const PR_LOOKUP_TIMEOUT_S: u64 = 10;
/// Default for `settings.LOOP_PR_LOOKUPS_PER_RUN` (`github.py:95`).
pub const DEFAULT_PR_LOOKUPS_PER_RUN: u32 = 15;
/// `github_client.py:38` — canonical host of parsed GitHub reviews.
pub const GITHUB_HOST: &str = "https://github.com";
/// `gitlab.py:47` — default self-hosted allowlist entry.
pub const GITLAB_COM_HOST: &str = "https://gitlab.com";
/// `gitlab.py:264` — merge-request path marker.
pub const GITLAB_MR_MARKER: &str = "/-/merge_requests/";

// ---------------------------------------------------------------------------
// Parsed review URL (registry + adapters)
// ---------------------------------------------------------------------------

/// `dtos.py:ParsedCodeReview` — the fields `get_pull_request_status`
/// reads: `provider`, `host_url`, `namespace`, `repo_name`,
/// `external_iid`, canonical `url`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedReview {
    pub provider: String,
    pub host_url: String,
    pub namespace: String,
    pub repo_name: String,
    pub external_iid: String,
    pub url: String,
}

/// Adapter order is registration order (`registry.py:12-15,37-42`):
/// GitHub first, then GitLab. A URL both could claim goes to GitHub.
/// The tool strips the input before parsing (`github.py:100`); the
/// GitLab adapter strips again internally (`gitlab.py:259`).
pub fn parse_code_review_url(url: &str, gitlab_hosts: &[String]) -> Option<ParsedReview> {
    let trimmed = super::tools_issues::py_strip(url);
    parse_github_review_url(trimmed).or_else(|| parse_gitlab_review_url(trimmed, gitlab_hosts))
}

/// `parse_github_pull_request_url` (`utils/github_client.py:264-275`)
/// over `_HTTPS_PR_RE` (`:259-261`):
/// `^https?://github\.com/([^/\s]+)/([^/\s]+?)/pull/(\d+)(?:/[^\s]*)?$`.
/// Owner and name are lowercased (`adapters/github.py:76-77`); the
/// canonical URL is rebuilt (`:84`). Non-`pull` paths (e.g. `/issues/`)
/// and non-github hosts return `None`.
pub fn parse_github_review_url(url: &str) -> Option<ParsedReview> {
    let url = super::tools_issues::py_strip(url);
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    if rest.chars().any(char::is_whitespace) {
        return None;
    }
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    let pull = parts.next()?;
    if owner.is_empty() || name.is_empty() || pull != "pull" {
        return None;
    }
    let number = parts.next()?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // `(?:/[^\s]*)?$` — any trailing path is allowed (whitespace
    // already rejected above); the name is non-greedy so no extra
    // validation applies to the tail. `int(group)` then `str()` strips
    // leading zeros with no width limit — replicate without a machine-
    // int round-trip so absurd digit runs behave identically.
    let stripped = number.trim_start_matches('0');
    let canonical_number = if stripped.is_empty() { "0" } else { stripped };
    let owner = owner.to_lowercase();
    let name = name.to_lowercase();
    Some(ParsedReview {
        provider: "github".to_owned(),
        host_url: GITHUB_HOST.to_owned(),
        namespace: owner.clone(),
        repo_name: name.clone(),
        external_iid: canonical_number.to_owned(),
        url: format!("{GITHUB_HOST}/{owner}/{name}/pull/{canonical_number}"),
    })
}

/// `GitLabAdapter._normalize_host` (`adapters/gitlab.py:37-45`): blank
/// input means gitlab.com; a bare host gains `https://`; scheme and
/// netloc lowercase.
pub fn normalize_gitlab_host(host: &str) -> String {
    let trimmed = super::tools_issues::py_strip(host).trim_end_matches('/');
    if trimmed.is_empty() {
        return GITLAB_COM_HOST.to_owned();
    }
    let with_scheme = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };
    match with_scheme.split_once("://") {
        Some((scheme, rest)) if scheme == "http" || scheme == "https" => {
            let netloc = rest.split('/').next().unwrap_or("");
            if netloc.is_empty() {
                with_scheme
            } else {
                format!("{}://{}", scheme.to_lowercase(), netloc.to_lowercase())
            }
        }
        _ => with_scheme,
    }
}

/// `GitLabAdapter.parse_code_review_url` (`adapters/gitlab.py:258-282`):
/// `http(s)` scheme, allowlisted host, `/-/merge_requests/` marker,
/// numeric iid, and a repo path with at least one `/` (namespace/name
/// split at the last `/`, `.git` suffix stripped — `_split_full_path`,
/// `:78-88`). The canonical URL is rebuilt (`:281`).
pub fn parse_gitlab_review_url(url: &str, gitlab_hosts: &[String]) -> Option<ParsedReview> {
    let candidate = super::tools_issues::py_strip(url);
    // Python matches against `urlparse` output, which splits `?query` and
    // `#fragment` off before the scheme/host/path checks — a pasted MR URL
    // with tracking params or a note anchor still parses there. Mirror the
    // split so such URLs parse here too (the GitHub regex needs no such
    // treatment: its optional tail already requires a `/` prefix, so a
    // `?`/`#` suffix fails matching on both sides).
    let candidate = candidate.split(['?', '#']).next().unwrap_or("");
    let (scheme, rest) = candidate.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let netloc = rest.split('/').next().unwrap_or("");
    // `_normalize_host(parsed.netloc)` — netloc has no scheme, so the
    // helper prefixes `https://`; an explicit `http://` host therefore
    // normalizes to `https://` and only matches an allowlist entry that
    // went through the same normalization.
    let host_url = normalize_gitlab_host(netloc);
    // `_allowed_hosts` (`gitlab.py:47-66`): gitlab.com, `GITLAB_HOST`,
    // and `GITLAB_ALLOWED_HOSTS` (comma string or list) — all through
    // the same normalization. The caller supplies the configured hosts
    // already split; gitlab.com is always present.
    let default_host = normalize_gitlab_host("");
    let allowed = std::iter::once(default_host.as_str())
        .chain(gitlab_hosts.iter().map(|host| host.as_str()))
        .map(normalize_gitlab_host)
        .any(|allowed| allowed == host_url);
    if !allowed {
        return None;
    }
    let path = &rest[netloc.len()..];
    let (repo_path, mr_part) = path.split_once(GITLAB_MR_MARKER)?;
    let mr_iid = mr_part.trim_matches('/').split('/').next().unwrap_or("");
    if mr_iid.is_empty() || !mr_iid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (namespace, name) = split_repo_path(repo_path)?;
    Some(ParsedReview {
        provider: "gitlab".to_owned(),
        host_url: host_url.clone(),
        namespace: namespace.clone(),
        repo_name: name.clone(),
        url: format!("{host_url}/{namespace}/{name}/-/merge_requests/{mr_iid}"),
        external_iid: mr_iid.to_owned(),
    })
}

fn split_repo_path(path: &str) -> Option<(String, String)> {
    let stripped = path
        .trim_matches('/')
        .strip_suffix(".git")
        .unwrap_or(path.trim_matches('/'));
    if stripped.is_empty() || !stripped.contains('/') {
        return None;
    }
    let (namespace, name) = stripped.rsplit_once('/')?;
    if namespace.is_empty() || name.is_empty() {
        return None;
    }
    Some((namespace.to_owned(), name.to_owned()))
}

// ---------------------------------------------------------------------------
// Request building
// ---------------------------------------------------------------------------

/// `github.py:104-110` — provider API URLs. The GitLab project path is
/// `quote(path, safe="")` (`urllib.parse.quote`): `/` becomes `%2F`
/// (Python `quote` always encodes `/` when `safe=""`; unreserved bytes
/// pass through — see [`percent_encode_path`]).
pub fn review_api_url(parsed: &ParsedReview) -> Option<String> {
    match parsed.provider.as_str() {
        "github" => Some(format!(
            "{GITHUB_API_HOST}/repos/{}/{}/pulls/{}",
            parsed.namespace, parsed.repo_name, parsed.external_iid
        )),
        "gitlab" => Some(format!(
            "{}/api/v4/projects/{}/merge_requests/{}",
            parsed.host_url,
            percent_encode_path(&format!("{}/{}", parsed.namespace, parsed.repo_name)),
            parsed.external_iid
        )),
        _ => None,
    }
}

/// `urllib.parse.quote(segment, safe="")`: UTF-8 bytes, unreserved
/// (`A-Z a-z 0-9 - _ . ~`) verbatim, everything else `%XX` uppercase hex.
pub fn percent_encode_path(segment: &str) -> String {
    const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.~";
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if UNRESERVED.contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `github.py:115-121` — headers. `Accept: application/json` always; the
/// token (when present) becomes `Authorization: Bearer` (+ the version
/// pin) for GitHub or `PRIVATE-TOKEN` for GitLab.
pub fn review_headers(provider: &str, token: Option<&str>) -> Vec<(String, String)> {
    let mut headers = vec![("Accept".to_owned(), "application/json".to_owned())];
    match (provider, token.filter(|token| !token.is_empty())) {
        ("github", Some(token)) => {
            headers.push(("X-GitHub-Api-Version".to_owned(), "2022-11-28".to_owned()));
            headers.push(("Authorization".to_owned(), format!("Bearer {token}")));
        }
        ("gitlab", Some(token)) => {
            headers.push(("PRIVATE-TOKEN".to_owned(), token.to_owned()));
        }
        _ => {}
    }
    headers
}

// ---------------------------------------------------------------------------
// Budget gate + unknown envelope
// ---------------------------------------------------------------------------

/// `github.py:35-36` — the never-raises envelope.
pub fn unknown_state(reason: &str) -> Value {
    json!({"state": "unknown", "reason": reason})
}

/// `github.py:94-98` — per-run budget guarding the unauthenticated
/// GitHub rate limit. Returns the new counter, or `None` when exhausted
/// (the caller answers `budget_exhausted` without consuming).
pub fn check_pr_budget(used: u32, cap: u32) -> Option<u32> {
    if used >= cap {
        None
    } else {
        Some(used + 1)
    }
}

// ---------------------------------------------------------------------------
// Response classification (github.py:138-165)
// ---------------------------------------------------------------------------

/// HTTP-status half (`github.py:138-145`): 404 / 403+429 / 451 map to
/// named reasons; any other non-200 becomes `http_{code}`. `None` means
/// "proceed to the payload half".
pub fn status_reason(status: u16) -> Option<String> {
    match status {
        200 => None,
        404 => Some("not_found".to_owned()),
        403 | 429 => Some("rate_limited".to_owned()),
        451 => Some("blocked".to_owned()),
        code => Some(format!("http_{code}")),
    }
}

/// Payload half (`github.py:147-158`): `merged`/`merged_at`/`"merged"`
/// win over `"closed"`; anything else (including a missing state) is
/// `open`. JSON parse failure is `bad_response` (decided by the caller).
pub fn classify_review_state(
    merged: bool,
    merged_at: Option<&str>,
    provider_state: Option<&str>,
) -> &'static str {
    if merged
        || merged_at.is_some_and(|value| !value.is_empty())
        || provider_state == Some("merged")
    {
        "merged"
    } else if provider_state == Some("closed") {
        "closed"
    } else {
        "open"
    }
}

/// Success shape (`github.py:159-165`): the canonical parsed URL, not the
/// caller's input string.
pub fn review_success(
    parsed: &ParsedReview,
    state: &str,
    title: Option<&str>,
    merged_at: Option<&str>,
) -> Value {
    json!({
        "state": state,
        "title": title,
        "merged_at": merged_at,
        "url": parsed.url,
        "provider": parsed.provider,
    })
}

// ---------------------------------------------------------------------------
// Token lookup (_find_token :48-82 + _maybe_decrypt :39-45)
// ---------------------------------------------------------------------------

/// Binding half (`github.py:52-65`): newest binding on a member project
/// whose repository matches provider + namespace/name case-insensitively;
/// the token is `provider_account.credential_config["token"]`.
/// (`*_iexact` → `UPPER()` comparisons, as in [`super::tools_issues`].)
/// The member-projects subquery is [`super::tools_issues::MEMBER_PROJECTS_SQL`]
/// inline, including its `projects.deleted_at` predicate; the outer
/// `deleted_at` predicate is the binding's own default manager. Joined
/// repository/account tables carry no manager filter, as in Django.
pub const BINDING_TOKEN_SQL: &str = "SELECT git_provider_accounts.credential_config \
     FROM git_repository_bindings \
     INNER JOIN git_repositories ON git_repositories.id = git_repository_bindings.repository_id \
     INNER JOIN git_provider_accounts \
     ON git_provider_accounts.id = git_repository_bindings.provider_account_id \
     WHERE git_repository_bindings.project_id IN (SELECT DISTINCT projects.id FROM projects \
     INNER JOIN project_members ON (project_members.project_id = projects.id \
     AND project_members.member_id = $1 AND project_members.is_active) \
     INNER JOIN workspaces ON workspaces.id = projects.workspace_id \
     WHERE workspaces.slug = $2 AND projects.deleted_at IS NULL) \
     AND git_repositories.provider = $3 \
     AND UPPER(git_repositories.namespace) = UPPER($4) \
     AND UPPER(git_repositories.name) = UPPER($5) \
     AND git_repository_bindings.deleted_at IS NULL \
     ORDER BY git_repository_bindings.created_at DESC LIMIT 1";

/// Sync fallback, GitHub only (`github.py:67-82`): newest sync on a member
/// project whose repository matches owner/name; `access_token` wins over
/// `token` (`creds.get("access_token") or creds.get("token")` — empty
/// strings fall through to the next key).
pub const SYNC_TOKEN_SQL: &str =
    "SELECT github_repository_syncs.credentials \
     FROM github_repository_syncs \
     INNER JOIN github_repositories ON github_repositories.id = github_repository_syncs.repository_id \
     WHERE github_repository_syncs.project_id IN (SELECT DISTINCT projects.id FROM projects \
     INNER JOIN project_members ON (project_members.project_id = projects.id \
     AND project_members.member_id = $1 AND project_members.is_active) \
     INNER JOIN workspaces ON workspaces.id = projects.workspace_id \
     WHERE workspaces.slug = $2 AND projects.deleted_at IS NULL) \
     AND UPPER(github_repositories.owner) = UPPER($3) \
     AND UPPER(github_repositories.name) = UPPER($4) \
     AND github_repository_syncs.deleted_at IS NULL \
     ORDER BY github_repository_syncs.created_at DESC LIMIT 1";

/// Picks `access_token`, else `token`, skipping empties
/// (`github.py:81`).
pub fn pick_sync_token(access_token: Option<&str>, token: Option<&str>) -> Option<String> {
    [access_token, token]
        .into_iter()
        .flatten()
        .find(|value| !value.is_empty())
        .map(str::to_owned)
}

/// `_maybe_decrypt` (`github.py:39-45`) over `decrypt_data`
/// (`license/utils/encryption.py:34-43`): `decrypt_data` catches
/// *everything* and returns `""` on failure (or for falsy input), so the
/// `except → return token` fallback in `_maybe_decrypt` is unreachable —
/// ported bug #1, preserved: undecryptable tokens become `""` (falsy, so
/// the request goes out unauthenticated) instead of falling back to the
/// stored value. `outcome=None` models "decrypt raised" (dead branch);
/// `Some(decrypted)` models the real return, including `""`.
pub fn maybe_decrypt(token: Option<&str>, outcome: Option<&str>) -> Option<String> {
    token.filter(|token| !token.is_empty())?;
    match outcome {
        Some(decrypted) => Some(decrypted.to_owned()),
        // Unreachable in Python (`decrypt_data` never raises): the
        // `return token` fallback. Preserved as `""` (what `decrypt_data`
        // actually yields on failure) rather than the stored token.
        None => Some(String::new()),
    }
}

// ---------------------------------------------------------------------------
// Tool schema
// ---------------------------------------------------------------------------

/// JSON Schema for `get_pull_request_status`, built by hand from the
/// Python signature (`github.py:85-91`) — see the `schemars` note in
/// [`super::tools_issues`].
pub fn tool_schema() -> Value {
    json!({
        "name": "get_pull_request_status",
        "description": "Check whether a GitHub pull request or GitLab merge request is merged. Pass a full PR/MR URL. Returns ``{\"state\": \"merged\"|\"open\"|\"closed\"|\"unknown\", ...}``. An unsupported URL, rate limit, or network error returns ``unknown``.",
        "parameters": {
            "type": "object",
            "properties": {
                "url": {"type": "string", "description": "Full PR/MR URL."},
            },
            "required": ["url"],
            "additionalProperties": false,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_gitlab_hosts() -> Vec<String> {
        Vec::new()
    }

    #[test]
    fn github_parse_basic_and_canonical() {
        let parsed = parse_code_review_url(
            "https://github.com/The-AI-Republic/pi-dash/pull/685",
            &no_gitlab_hosts(),
        )
        .expect("parses");
        assert_eq!(parsed.provider, "github");
        assert_eq!(parsed.namespace, "the-ai-republic");
        assert_eq!(parsed.repo_name, "pi-dash");
        assert_eq!(parsed.external_iid, "685");
        assert_eq!(
            parsed.url,
            "https://github.com/the-ai-republic/pi-dash/pull/685"
        );
    }

    #[test]
    fn github_parse_rejects_non_pull_and_hosts() {
        assert!(parse_github_review_url("https://github.com/o/n/issues/12").is_none());
        assert!(parse_github_review_url("https://example.com/o/n/pull/12").is_none());
        assert!(parse_github_review_url("https://github.com/o/pull/abc").is_none());
        assert!(parse_github_review_url("https://github.com/o/n/pull/").is_none());
        assert!(parse_github_review_url("").is_none());
    }

    #[test]
    fn github_parse_trailing_path_and_zero_strip() {
        let parsed = parse_github_review_url("https://github.com/o/n/pull/007/files").unwrap();
        assert_eq!(parsed.external_iid, "7");
        assert_eq!(parsed.url, "https://github.com/o/n/pull/7");
        // Padded input with surrounding whitespace (the tool strips first).
        assert!(
            parse_code_review_url("  https://github.com/o/n/pull/3  ", &no_gitlab_hosts())
                .is_some()
        );
    }

    #[test]
    fn gitlab_parse_subgroup_and_git_suffix() {
        let parsed = parse_code_review_url(
            "https://gitlab.com/group/sub/repo/-/merge_requests/42",
            &no_gitlab_hosts(),
        )
        .expect("parses");
        assert_eq!(parsed.provider, "gitlab");
        assert_eq!(parsed.namespace, "group/sub");
        assert_eq!(parsed.repo_name, "repo");
        assert_eq!(parsed.external_iid, "42");
        assert_eq!(
            parsed.url,
            "https://gitlab.com/group/sub/repo/-/merge_requests/42"
        );
        let suffixed =
            parse_gitlab_review_url("https://gitlab.com/group/repo.git/-/merge_requests/9", &[])
                .unwrap();
        assert_eq!(suffixed.repo_name, "repo");
    }

    #[test]
    fn gitlab_parse_rejects_scheme_host_and_iid() {
        // Wrong scheme.
        assert!(parse_gitlab_review_url("ssh://gitlab.com/g/r/-/merge_requests/1", &[]).is_none());
        // Unlisted self-hosted instance.
        assert!(
            parse_gitlab_review_url("https://git.example.com/g/r/-/merge_requests/1", &[])
                .is_none()
        );
        // Listed self-hosted instance (any normalization accepted).
        let parsed = parse_gitlab_review_url(
            "https://git.example.com/g/r/-/merge_requests/1",
            &["git.example.com".to_owned()],
        )
        .unwrap();
        assert_eq!(parsed.host_url, "https://git.example.com");
        // Non-numeric iid and missing marker.
        assert!(
            parse_gitlab_review_url("https://gitlab.com/g/r/-/merge_requests/x", &[]).is_none()
        );
        assert!(parse_gitlab_review_url("https://gitlab.com/g/r/merge_requests/1", &[]).is_none());
    }

    #[test]
    fn api_urls_match_python_shapes() {
        let github = parse_github_review_url("https://github.com/o/n/pull/12").unwrap();
        assert_eq!(
            review_api_url(&github).as_deref(),
            Some("https://api.github.com/repos/o/n/pulls/12")
        );
        let gitlab =
            parse_gitlab_review_url("https://gitlab.com/g/sub/r/-/merge_requests/3", &[]).unwrap();
        assert_eq!(
            review_api_url(&gitlab).as_deref(),
            Some("https://gitlab.com/api/v4/projects/g%2Fsub%2Fr/merge_requests/3")
        );
    }

    #[test]
    fn percent_encode_matches_quote_safe_empty() {
        assert_eq!(percent_encode_path("g/sub r~x"), "g%2Fsub%20r~x");
        assert_eq!(percent_encode_path("abc-_.~09AZaz"), "abc-_.~09AZaz");
    }

    #[test]
    fn headers_matrix() {
        let anon = review_headers("github", None);
        assert_eq!(anon.len(), 1);
        let github = review_headers("github", Some("tok"));
        assert!(github
            .iter()
            .any(|(k, v)| k == "X-GitHub-Api-Version" && v == "2022-11-28"));
        assert!(github
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "Bearer tok"));
        // Empty tokens behave as absent.
        assert_eq!(review_headers("github", Some("")).len(), 1);
        let gitlab = review_headers("gitlab", Some("tok"));
        assert!(gitlab
            .iter()
            .any(|(k, v)| k == "PRIVATE-TOKEN" && v == "tok"));
        assert!(!gitlab.iter().any(|(k, _)| k == "Authorization"));
    }

    #[test]
    fn budget_gate_saturates_at_cap() {
        assert_eq!(check_pr_budget(14, DEFAULT_PR_LOOKUPS_PER_RUN), Some(15));
        assert_eq!(check_pr_budget(15, DEFAULT_PR_LOOKUPS_PER_RUN), None);
        assert_eq!(check_pr_budget(99, DEFAULT_PR_LOOKUPS_PER_RUN), None);
    }

    #[test]
    fn status_reasons_match_python_branches() {
        assert_eq!(status_reason(200), None);
        assert_eq!(status_reason(404).as_deref(), Some("not_found"));
        assert_eq!(status_reason(403).as_deref(), Some("rate_limited"));
        assert_eq!(status_reason(429).as_deref(), Some("rate_limited"));
        assert_eq!(status_reason(451).as_deref(), Some("blocked"));
        assert_eq!(status_reason(500).as_deref(), Some("http_500"));
        assert_eq!(status_reason(301).as_deref(), Some("http_301"));
    }

    #[test]
    fn review_state_precedence() {
        assert_eq!(classify_review_state(true, None, Some("open")), "merged");
        assert_eq!(
            classify_review_state(false, Some("2026-01-01"), Some("open")),
            "merged"
        );
        assert_eq!(classify_review_state(false, None, Some("merged")), "merged");
        assert_eq!(classify_review_state(false, None, Some("closed")), "closed");
        assert_eq!(classify_review_state(false, None, Some("open")), "open");
        // Missing state (or anything else) reads as open.
        assert_eq!(classify_review_state(false, None, None), "open");
        assert_eq!(
            classify_review_state(false, Some(""), Some("locked")),
            "open"
        );
    }

    #[test]
    fn success_shape_uses_canonical_url() {
        let parsed = parse_github_review_url("https://github.com/O/N/pull/12").unwrap();
        let value = review_success(&parsed, "merged", Some("T"), Some("2026-01-01T00:00:00Z"));
        assert_eq!(value["state"], "merged");
        assert_eq!(value["url"], "https://github.com/o/n/pull/12");
        assert_eq!(value["provider"], "github");
        assert_eq!(value["title"], "T");
        // The unknown envelope carries only state + reason.
        assert_eq!(
            unknown_state("unsupported_url")["reason"],
            "unsupported_url"
        );
    }

    #[test]
    fn sync_token_prefers_access_token_skipping_empties() {
        assert_eq!(pick_sync_token(Some("a"), Some("b")).as_deref(), Some("a"));
        assert_eq!(pick_sync_token(Some(""), Some("b")).as_deref(), Some("b"));
        assert_eq!(pick_sync_token(None, Some("")).as_deref(), None);
        assert_eq!(pick_sync_token(None, None), None);
    }

    #[test]
    fn decrypt_failure_yields_empty_string_not_the_token() {
        // Ported bug #1: `decrypt_data` never raises (returns ""), so the
        // `except → return token` fallback is dead — undecryptable tokens
        // become "" and the request goes out unauthenticated.
        assert_eq!(
            maybe_decrypt(Some("stored"), Some("plain")).as_deref(),
            Some("plain")
        );
        assert_eq!(maybe_decrypt(Some("stored"), Some("")).as_deref(), Some(""));
        assert_eq!(maybe_decrypt(Some("stored"), None).as_deref(), Some(""));
        assert_eq!(maybe_decrypt(None, Some("plain")), None);
        assert_eq!(maybe_decrypt(Some(""), Some("plain")), None);
    }

    #[test]
    fn schema_names_the_tool_and_its_only_param() {
        let schema = tool_schema();
        assert_eq!(schema["name"], "get_pull_request_status");
        assert_eq!(schema["parameters"]["required"], json!(["url"]));
    }

    #[test]
    fn gitlab_query_and_fragment_still_parse() {
        // Python matches against `urlparse` output, where `?query` and
        // `#fragment` never reach the path checks.
        for url in [
            "https://gitlab.com/g/r/-/merge_requests/42?foo=bar",
            "https://gitlab.com/g/r/-/merge_requests/42#note_1",
            "https://gitlab.com/g/r/-/merge_requests/42/?foo=bar",
        ] {
            let parsed = parse_gitlab_review_url(url, &[]).expect("parses");
            assert_eq!(parsed.external_iid, "42", "{url}");
            assert_eq!(
                parsed.url, "https://gitlab.com/g/r/-/merge_requests/42",
                "{url}"
            );
        }
        // The GitHub regex needs no such treatment: its optional tail
        // requires a `/` prefix, so suffixes fail on both sides alike.
        assert!(parse_github_review_url("https://github.com/o/n/pull/12?x=1").is_none());
        assert!(parse_github_review_url("https://github.com/o/n/pull/12/files").is_some());
    }

    #[test]
    fn token_subqueries_exclude_deleted_projects() {
        assert!(BINDING_TOKEN_SQL.contains("projects.deleted_at IS NULL"));
        assert!(SYNC_TOKEN_SQL.contains("projects.deleted_at IS NULL"));
        assert!(BINDING_TOKEN_SQL.contains("git_repository_bindings.deleted_at IS NULL"));
        assert!(SYNC_TOKEN_SQL.contains("github_repository_syncs.deleted_at IS NULL"));
    }
}
