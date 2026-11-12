//! OAuth token/userinfo exchange error mapping (D-17 services layer).
//!
//! Port of the HTTP-exchange half of
//! `apps/api/pi_dash/authentication/adapter/oauth.py:75-100`
//! (`OauthAdapter.get_user_token`, `OauthAdapter.get_user_response`), plus
//! the transport-error branches of the github helpers that live behind the
//! same mapping (`provider/oauth/github.py:108-143`: `__get_email` and the
//! org-membership gate in `set_user_data`).
//!
//! This module is transport-agnostic on purpose: the request *shapes* (POST
//! the token URL with `data` + `headers`; GET the userinfo URL with a
//! `Bearer` header) are pinned as pure spec constructors, and every
//! `requests.RequestException` branch maps to an
//! [`AuthenticationException`] via [`map_exchange_error`] and friends. The
//! handlers issue owns the actual HTTP client; the pure token/user-data
//! mappers live in [`super::providers`].
//!
//! Fixture ids: AUTHOAUTH-F4 request shapes and github branches (fixtures under
//! `rust-api/fixtures/auth_oauth/`); codes resolve through [`super::error`].
//!
//! # Error mapping (every branch below warns, then raises)
//!
//! | Python site | failure | exception |
//! | --- | --- | --- |
//! | `get_user_token` (`oauth.py:75-84`) | `RequestException` on POST or `raise_for_status` | provider `*_OAUTH_PROVIDER_ERROR` |
//! | `get_user_response` (`oauth.py:86-100`) | `RequestException` on GET or `raise_for_status` | provider `*_OAUTH_PROVIDER_ERROR` |
//! | github `__get_email` (`github.py:108-135`) | non-list payload, no primary email, or `RequestException` | `GITHUB_OAUTH_PROVIDER_ERROR` (5120) in all three cases |
//! | github org gate (`github.py:137-143` + `set_user_data`) | org set and membership status != 200 | `GITHUB_USER_NOT_IN_ORG` (5122) |
//!
//! `error_message` is always `str(code)`: the code *name*, not the number
//! (`oauth.py:83,99,125-134`).

use super::error::{authentication_error_code, error_code_by_name, AuthenticationException};
use super::providers::{github_membership_url, is_org_member};

// ---------------------------------------------------------------------------
// Request shapes
// ---------------------------------------------------------------------------

/// HTTP method of the token fetch (`requests.post(token_url, data, headers)`,
/// `oauth.py:78`).
pub const TOKEN_FETCH_METHOD: &str = "POST";

/// HTTP method of the userinfo fetch (`requests.get(userinfo_url, headers)`,
/// `oauth.py:89`).
pub const USERINFO_FETCH_METHOD: &str = "GET";

/// Emails-endpoint fetch method (`requests.get(emails_url, headers)`,
/// `github.py:112`; gitea `gitea.py:116-147`).
pub const EMAILS_FETCH_METHOD: &str = "GET";

/// `{"Authorization": f"Bearer {token_data['access_token']}"}`
/// (`oauth.py:88`; the org gate reuses the same header, `github.py:138`).
pub fn bearer_headers(access_token: &str) -> Vec<(String, String)> {
    vec![("Authorization".to_owned(), format!("Bearer {access_token}"))]
}

/// `{"Accept": "application/json"}` merged into the emails GET headers
/// (`github.py:112`; gitea equivalent).
pub fn json_accept_headers() -> Vec<(String, String)> {
    vec![("Accept".to_owned(), "application/json".to_owned())]
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

/// Build the `AuthenticationException` for a provider error-code *name*:
/// `error_code` looks up the number, `error_message` is the name itself
/// (`error_message=str(code)`).
fn exception_for_name(name: &str) -> AuthenticationException {
    AuthenticationException::new(
        error_code_by_name(name).unwrap_or(5999),
        name.to_owned(),
        Vec::new(),
    )
}

/// `except requests.RequestException` in `get_user_token` (`oauth.py:80-84`)
/// and `get_user_response` (`oauth.py:92-100`): warn, then raise
/// `AuthenticationException(authentication_error_code(), str(code))`.
/// Covers transport failures and `raise_for_status` rejections alike —
/// Python catches both as `RequestException`.
pub fn map_exchange_error(provider: &str) -> AuthenticationException {
    exception_for_name(authentication_error_code(provider))
}

/// All three github `__get_email` failure branches (`github.py:108-135`):
/// a non-list payload, no truthy-`primary` entry, or a
/// `RequestException` — every one raises `GITHUB_OAUTH_PROVIDER_ERROR`.
/// The entry *selection* itself is [`super::providers::github_primary_email`];
/// this maps its [`super::providers::GithubEmailError`] variants (plus the
/// transport failure, which has no selector value) to the exception.
pub fn map_github_email_error() -> AuthenticationException {
    exception_for_name("GITHUB_OAUTH_PROVIDER_ERROR")
}

/// The org gate denial (`github.py:137-143` + `set_user_data`): only when
/// `organization_id` is set and the membership check fails, warn and raise
/// `GITHUB_USER_NOT_IN_ORG` (5122) — *not* the provider error (5120).
pub fn map_github_org_denial() -> AuthenticationException {
    exception_for_name("GITHUB_USER_NOT_IN_ORG")
}

/// Membership verdict feeding the org gate: member iff the membership GET
/// returns 200 (`github.py:143`). Re-exported predicate so handlers branch
/// on one name; see [`super::providers::is_org_member`].
pub fn github_member_by_status(status_code: u16) -> bool {
    is_org_member(status_code)
}

/// Membership URL the gate GETs
/// (`f"{org_membership_url}/{organization_id}/memberships/{login}"`,
/// `github.py:139-142`); re-exported for the handlers layer.
pub fn github_org_membership_url(
    org_membership_url: &str,
    organization_id: &str,
    github_username: &str,
) -> String {
    github_membership_url(org_membership_url, organization_id, github_username)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> serde_json::Value {
        let path = format!(
            "{}/../../fixtures/auth_oauth/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    // -- request shapes (AUTHOAUTH-F4 base_adapter) ----------------------------

    #[test]
    fn fetch_shapes_match_fixture() {
        let f4 = fixture("F4_provider_token_user_data.golden.json");
        let base = &f4["base_adapter"];
        assert_eq!(TOKEN_FETCH_METHOD, "POST");
        assert!(base["get_user_token"]["method"]
            .as_str()
            .expect("token method")
            .starts_with("requests.post(token_url"));
        assert_eq!(USERINFO_FETCH_METHOD, "GET");
        assert!(base["get_user_response"]["method"]
            .as_str()
            .expect("userinfo method")
            .starts_with("requests.get(userinfo_url)"));
        // `{"Authorization": "Bearer {token_data.access_token}"}`.
        assert_eq!(
            base["get_user_response"]["headers"],
            serde_json::json!({"Authorization": "Bearer {token_data.access_token}"})
        );
        assert_eq!(
            bearer_headers("abc"),
            vec![("Authorization".to_owned(), "Bearer abc".to_owned())]
        );
    }

    // -- provider error mapping (oauth.py:49-59,75-100) ------------------------

    #[test]
    fn exchange_error_codes_per_provider() {
        // `authentication_error_code()` per provider, then the numeric code
        // from the F5 table; message is the name (`str(code)`).
        for (provider, name, code) in [
            ("google", "GOOGLE_OAUTH_PROVIDER_ERROR", 5115),
            ("github", "GITHUB_OAUTH_PROVIDER_ERROR", 5120),
            ("gitlab", "GITLAB_OAUTH_PROVIDER_ERROR", 5121),
            ("gitea", "GITEA_OAUTH_PROVIDER_ERROR", 5123),
        ] {
            let exc = map_exchange_error(provider);
            assert_eq!(exc.error_code, code, "{provider} code");
            assert_eq!(exc.error_message, name, "{provider} message is str(code)");
        }
        let unknown = map_exchange_error("okta");
        assert_eq!(
            unknown.error_code, 5104,
            "unknown provider falls back to OAUTH_NOT_CONFIGURED"
        );
        assert_eq!(unknown.error_message, "OAUTH_NOT_CONFIGURED");
    }

    #[test]
    fn exchange_errors_match_fixture_branches() {
        let f4 = fixture("F4_provider_token_user_data.golden.json");
        let base = &f4["base_adapter"];
        for key in ["get_user_token", "get_user_response"] {
            assert!(
                base[key]["on_RequestException"]
                    .as_str()
                    .expect("branch")
                    .contains("AuthenticationException"),
                "{key} raises AuthenticationException"
            );
        }
        // get_user_response logs the headers extra (`oauth.py:93-98`); the
        // shape above carries them, so handlers can reproduce the log call.
        assert_eq!(
            bearer_headers("tok"),
            vec![("Authorization".to_owned(), "Bearer tok".to_owned())]
        );
    }

    // -- github email + org branches (github.py:108-143) -----------------------

    #[test]
    fn github_email_failures_map_to_provider_error() {
        let f4 = fixture("F4_provider_token_user_data.golden.json");
        let branch = &f4["github"]["get_email_private"];
        assert!(
            branch["error_when"]
                .as_str()
                .expect("error_when")
                .contains("GITHUB_OAUTH_PROVIDER_ERROR"),
            "non-list + no-primary + RequestException share one code"
        );
        let exc = map_github_email_error();
        assert_eq!(exc.error_code, 5120);
        assert_eq!(exc.error_message, "GITHUB_OAUTH_PROVIDER_ERROR");
    }

    #[test]
    fn github_org_branch_maps_to_not_in_org() {
        let f4 = fixture("F4_provider_token_user_data.golden.json");
        let gate = &f4["github"]["org_gate"];
        assert!(
            gate["fail"]
                .as_str()
                .expect("fail")
                .contains("GITHUB_USER_NOT_IN_ORG(5122)"),
            "org denial is 5122, not the 5120 provider error"
        );
        assert!(
            gate["fail"]
                .as_str()
                .expect("fail")
                .contains("only when organization_id set"),
            "gate only applies with an org configured"
        );
        let denial = map_github_org_denial();
        assert_eq!(denial.error_code, 5122);
        assert_eq!(denial.error_message, "GITHUB_USER_NOT_IN_ORG");
        // ...while the same provider's exchange failure stays 5120.
        assert_ne!(denial.error_code, map_exchange_error("github").error_code);
        // Member iff status 200 (github.py:143).
        assert!(github_member_by_status(200));
        for status in [201, 204, 400, 403, 404, 500] {
            assert!(
                !github_member_by_status(status),
                "status {status} is not a member"
            );
        }
    }

    #[test]
    fn membership_url_shape() {
        assert_eq!(
            github_org_membership_url("https://api.github.com/orgs", "o1", "octo"),
            "https://api.github.com/orgs/o1/memberships/octo"
        );
    }
}
