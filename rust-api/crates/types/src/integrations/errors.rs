//! Git provider error hierarchy and adapter contract (D-05, stage 5).
//!
//! Ports `apps/api/pi_dash/integrations/git/adapters/base.py:23-86`:
//! `GitProviderError` with its `Auth` / `Permission` / `NotFound` subclasses
//! plus the 14-method `GitProviderAdapter` Protocol.
//!
//! Hierarchy and status mapping (callers depend on both):
//!
//! * Every variant is a [`GitProviderError`], matching Python where every
//!   subclass is an instance of the base (`except GitProviderError`
//!   catches all three subclasses).
//! * [`GitProviderError::status_code`] encodes the view mapping at
//!   `app/views/integration/git.py:39-47`: auth → 401, permission → 403,
//!   not-found → 404, anything else → 400. The views also fall back to a
//!   per-kind default message when `str(exc)` is empty
//!   (`"Provider rejected this credential"`,
//!   `"Provider credential lacks permission"`,
//!   `"Repository not found or inaccessible"`); those defaults live with the
//!   handlers, not here.
//!
//! The [`GitProviderAdapter`] trait is synchronous: the Python adapters are
//! blocking, and any async wrapper belongs to the services layer. Adapter
//! implementations land in PIDASHCONV-141 (GitHub) and PIDASHCONV-143
//! (GitLab); the DTO types come from [`super::dtos`].

use serde_json::Value;

use super::dtos::{
    GitProviderCapabilities, ParsedCodeReview, ParsedRepository, ProviderWebhookEvent,
    RemoteCodeReview, RemoteComment, RemoteIssue, RemoteRepository, RepositoryPage,
};

/// Provider integration failure (`base.py:23-36`).
///
/// Each variant carries the message Python would put in `Exception.args`
/// (surfaced via `str(exc)`); it may be empty, in which case the views
/// substitute their per-kind default.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GitProviderError {
    /// Base failure (`GitProviderError` raised directly) → HTTP 400.
    #[error("{0}")]
    General(String),
    /// Provider rejected the configured credential → HTTP 401.
    #[error("{0}")]
    Auth(String),
    /// Credential lacks permission for the requested action → HTTP 403.
    #[error("{0}")]
    Permission(String),
    /// Resource not found or not visible to the credential → HTTP 404.
    #[error("{0}")]
    NotFound(String),
}

impl GitProviderError {
    /// HTTP status for this error, per `app/views/integration/git.py:39-47`.
    pub fn status_code(&self) -> u16 {
        match self {
            GitProviderError::General(_) => 400,
            GitProviderError::Auth(_) => 401,
            GitProviderError::Permission(_) => 403,
            GitProviderError::NotFound(_) => 404,
        }
    }

    /// The `str(exc)` message (possibly empty; see module docs).
    pub fn message(&self) -> &str {
        match self {
            GitProviderError::General(msg)
            | GitProviderError::Auth(msg)
            | GitProviderError::Permission(msg)
            | GitProviderError::NotFound(msg) => msg,
        }
    }

    /// `isinstance(exc, GitProviderAuthError)`.
    pub fn is_auth(&self) -> bool {
        matches!(self, GitProviderError::Auth(_))
    }

    /// `isinstance(exc, GitProviderPermissionError)`.
    pub fn is_permission(&self) -> bool {
        matches!(self, GitProviderError::Permission(_))
    }

    /// `isinstance(exc, GitProviderNotFoundError)`.
    pub fn is_not_found(&self) -> bool {
        matches!(self, GitProviderError::NotFound(_))
    }
}

/// Adapter contract (`GitProviderAdapter` Protocol, `base.py:39-86`).
///
/// `key` / `display_name` / `code_review_term` are associated constants
/// (the Protocol's class attributes). `credential` is the merged provider
/// credential dict (`services.account_credential`); it crosses as a JSON
/// value because its keys vary per `auth_type`. `headers` for webhook
/// normalization crosses as a JSON object of strings. Network-touching
/// methods return `Result`; the two URL parsers are infallible and return
/// `None` for non-matching input, exactly like Python.
pub trait GitProviderAdapter {
    /// Registration key (`"github"`, `"gitlab"`).
    const KEY: &'static str;
    /// Human name (`"GitHub"`, `"GitLab"`).
    const DISPLAY_NAME: &'static str;
    /// Review noun (`"pull request"`, `"merge request"`).
    const CODE_REVIEW_TERM: &'static str;

    /// `parse_repo_url` (`base.py:44-45`).
    fn parse_repo_url(&self, url: &str) -> Option<ParsedRepository>;

    /// `parse_code_review_url` (`base.py:47-48`).
    fn parse_code_review_url(&self, url: &str) -> Option<ParsedCodeReview>;

    /// `verify_provider_account` (`base.py:50-51`).
    fn verify_provider_account(&self, credential: &Value) -> Result<Value, GitProviderError>;

    /// `credential_capabilities` (`base.py:53-54`).
    fn credential_capabilities(
        &self,
        credential: &Value,
    ) -> Result<GitProviderCapabilities, GitProviderError>;

    /// `list_repositories` (`base.py:56-57`; Python default `page=1` is
    /// passed explicitly — Rust has no default arguments).
    fn list_repositories(
        &self,
        credential: &Value,
        page: i64,
    ) -> Result<RepositoryPage, GitProviderError>;

    /// `get_repository` (`base.py:59-60`).
    fn get_repository(
        &self,
        credential: &Value,
        parsed: &ParsedRepository,
    ) -> Result<RemoteRepository, GitProviderError>;

    /// `list_open_issues` (`base.py:62-63`).
    fn list_open_issues(
        &self,
        credential: &Value,
        repository: &RemoteRepository,
    ) -> Result<Vec<RemoteIssue>, GitProviderError>;

    /// `list_issue_comments` (`base.py:65-71`).
    fn list_issue_comments(
        &self,
        credential: &Value,
        repository: &RemoteRepository,
        issue_iid: &str,
    ) -> Result<Vec<RemoteComment>, GitProviderError>;

    /// `post_issue_comment` (`base.py:73-79`).
    fn post_issue_comment(
        &self,
        credential: &Value,
        repository: &RemoteRepository,
        issue_iid: &str,
        body: &str,
    ) -> Result<RemoteComment, GitProviderError>;

    /// `get_code_review` (`base.py:82-83`).
    fn get_code_review(
        &self,
        credential: &Value,
        parsed: &ParsedCodeReview,
    ) -> Result<RemoteCodeReview, GitProviderError>;

    /// `normalize_webhook` (`base.py:85-86`).
    fn normalize_webhook(
        &self,
        raw_body: &[u8],
        headers: &Value,
    ) -> Result<ProviderWebhookEvent, GitProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal stub proving the trait has all fourteen members with
    /// compatible shapes (object-safe is not required; adapters are
    /// used as concrete types, as in Python).
    struct Stub;

    impl GitProviderAdapter for Stub {
        const KEY: &'static str = "stub";
        const DISPLAY_NAME: &'static str = "Stub";
        const CODE_REVIEW_TERM: &'static str = "review";

        fn parse_repo_url(&self, _url: &str) -> Option<ParsedRepository> {
            None
        }

        fn parse_code_review_url(&self, _url: &str) -> Option<ParsedCodeReview> {
            None
        }

        fn verify_provider_account(&self, credential: &Value) -> Result<Value, GitProviderError> {
            Ok(credential.clone())
        }

        fn credential_capabilities(
            &self,
            _credential: &Value,
        ) -> Result<GitProviderCapabilities, GitProviderError> {
            Ok(GitProviderCapabilities {
                read_repositories: true,
                read_issues: true,
                write_comments: false,
                manage_webhooks: false,
                clone: false,
            })
        }

        fn list_repositories(
            &self,
            _credential: &Value,
            page: i64,
        ) -> Result<RepositoryPage, GitProviderError> {
            Ok(RepositoryPage {
                repositories: Vec::new(),
                page,
                has_next_page: false,
            })
        }

        fn get_repository(
            &self,
            _credential: &Value,
            parsed: &ParsedRepository,
        ) -> Result<RemoteRepository, GitProviderError> {
            Err(GitProviderError::NotFound(parsed.full_name.clone()))
        }

        fn list_open_issues(
            &self,
            _credential: &Value,
            _repository: &RemoteRepository,
        ) -> Result<Vec<RemoteIssue>, GitProviderError> {
            Ok(Vec::new())
        }

        fn list_issue_comments(
            &self,
            _credential: &Value,
            _repository: &RemoteRepository,
            _issue_iid: &str,
        ) -> Result<Vec<RemoteComment>, GitProviderError> {
            Ok(Vec::new())
        }

        fn post_issue_comment(
            &self,
            _credential: &Value,
            _repository: &RemoteRepository,
            _issue_iid: &str,
            _body: &str,
        ) -> Result<RemoteComment, GitProviderError> {
            Err(GitProviderError::Permission("denied".into()))
        }

        fn get_code_review(
            &self,
            _credential: &Value,
            _parsed: &ParsedCodeReview,
        ) -> Result<RemoteCodeReview, GitProviderError> {
            Err(GitProviderError::Auth("revoked".into()))
        }

        fn normalize_webhook(
            &self,
            _raw_body: &[u8],
            _headers: &Value,
        ) -> Result<ProviderWebhookEvent, GitProviderError> {
            Err(GitProviderError::General("bad body".into()))
        }
    }

    #[test]
    fn status_codes_match_view_mapping() {
        // app/views/integration/git.py:39-47.
        assert_eq!(GitProviderError::Auth("x".into()).status_code(), 401);
        assert_eq!(GitProviderError::Permission("x".into()).status_code(), 403);
        assert_eq!(GitProviderError::NotFound("x".into()).status_code(), 404);
        assert_eq!(GitProviderError::General("x".into()).status_code(), 400);
    }

    #[test]
    fn hierarchy_predicates_match_isinstance_checks() {
        let auth = GitProviderError::Auth("rejected".into());
        let perm = GitProviderError::Permission("denied".into());
        let missing = GitProviderError::NotFound("gone".into());
        let general = GitProviderError::General("boom".into());
        assert!(auth.is_auth() && !auth.is_permission() && !auth.is_not_found());
        assert!(perm.is_permission() && !perm.is_auth() && !perm.is_not_found());
        assert!(missing.is_not_found() && !missing.is_auth() && !missing.is_permission());
        assert!(!general.is_auth() && !general.is_permission() && !general.is_not_found());
        // Every variant is still a GitProviderError (except GitProviderError
        // catches the subclasses): the enum makes this structural.
        for err in [&auth, &perm, &missing, &general] {
            let _: &GitProviderError = err;
        }
    }

    #[test]
    fn display_preserves_str_exc_message() {
        assert_eq!(
            GitProviderError::Auth("bad token".into()).to_string(),
            "bad token"
        );
        assert_eq!(GitProviderError::General(String::new()).to_string(), "");
        assert_eq!(GitProviderError::NotFound("r".into()).message(), "r");
    }

    #[test]
    fn stub_exercises_all_fourteen_trait_members() {
        let stub = Stub;
        assert_eq!(Stub::KEY, "stub");
        assert_eq!(Stub::DISPLAY_NAME, "Stub");
        assert_eq!(Stub::CODE_REVIEW_TERM, "review");
        assert_eq!(stub.parse_repo_url("https://example.test/x/y"), None);
        assert_eq!(stub.parse_code_review_url("https://example.test/x"), None);
        let cred = serde_json::json!({"auth_type": "token"});
        assert_eq!(stub.verify_provider_account(&cred).expect("echo"), cred);
        let caps = stub.credential_capabilities(&cred).expect("caps");
        assert!(caps.read_repositories && caps.read_issues && !caps.clone);
        let page = stub.list_repositories(&cred, 1).expect("page");
        assert_eq!((page.page, page.has_next_page), (1, false));
        let parsed = ParsedRepository {
            provider: "stub".into(),
            host_url: "".into(),
            namespace: "a".into(),
            name: "b".into(),
            full_name: "a/b".into(),
            clone_url: "".into(),
        };
        assert!(stub.get_repository(&cred, &parsed).is_err());
        let repo = RemoteRepository {
            provider: "stub".into(),
            external_id: "1".into(),
            namespace: "a".into(),
            name: "b".into(),
            full_name: "a/b".into(),
            web_url: "".into(),
            clone_url_http: String::new(),
            clone_url_ssh: String::new(),
            default_branch: String::new(),
            is_private: false,
            metadata: Value::Object(Default::default()),
        };
        assert!(stub
            .list_open_issues(&cred, &repo)
            .expect("issues")
            .is_empty());
        assert!(stub
            .list_issue_comments(&cred, &repo, "7")
            .expect("comments")
            .is_empty());
        assert_eq!(
            stub.post_issue_comment(&cred, &repo, "7", "hi"),
            Err(GitProviderError::Permission("denied".into()))
        );
        let review = ParsedCodeReview {
            provider: "stub".into(),
            host_url: "".into(),
            namespace: "a".into(),
            repo_name: "b".into(),
            external_iid: "1".into(),
            url: "".into(),
        };
        assert_eq!(
            stub.get_code_review(&cred, &review),
            Err(GitProviderError::Auth("revoked".into()))
        );
        assert_eq!(
            stub.normalize_webhook(b"{}", &Value::Object(Default::default())),
            Err(GitProviderError::General("bad body".into()))
        );
    }
}
