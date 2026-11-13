#![forbid(unsafe_code)]

//! AWS credential resolution for the hand-rolled SigV4 clients.
//!
//! `boto3` resolves credentials through a chain: environment variables
//! first, then (among others) the EC2 instance role served by the
//! instance metadata service (IMDS). The Django deployment relies on the
//! instance-role step: production containers carry no static keys and
//! boto3 picks the role up on its own. The Rust KMS transport used to read
//! the environment only, so on the same host every KMS call failed with
//! "AWS credentials are not configured" before it reached AWS.
//!
//! This module restores the two steps that matter for the deployments:
//!
//! 1. `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY` (+ optional
//!    `AWS_SESSION_TOKEN`) — static keys, LocalStack, local dev.
//! 2. The EC2 instance role over IMDSv2: `PUT /latest/api/token`, then
//!    `GET /latest/meta-data/iam/security-credentials/` for the role name
//!    and `GET .../<role>` for temporary credentials. The result is cached
//!    process-wide and refreshed [`REFRESH_MARGIN_SECS`] before it
//!    expires.
//!
//! Exactly one of the two static keys set is an error, as boto3's
//! `PartialCredentialsError`: falling through to the role would sign as
//! a principal the operator never configured.
//!
//! `AWS_EC2_METADATA_DISABLED=true` skips step 2 (boto3's switch), and
//! `AWS_EC2_METADATA_SERVICE_ENDPOINT` overrides the IMDS base URL (also
//! boto3's), which the tests use to point at a local fake.
//!
//! Containers reach IMDSv2 only when the instance's metadata hop limit is
//! at least 2; the production hosts are configured that way.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

/// IMDS base URL when `AWS_EC2_METADATA_SERVICE_ENDPOINT` is unset.
const DEFAULT_IMDS_ENDPOINT: &str = "http://169.254.169.254";
/// Session-token lifetime requested from IMDSv2 (the 6 h maximum; the
/// token is only used for the three calls of one fetch).
const IMDS_TOKEN_TTL_SECS: &str = "21600";
/// Refresh role credentials this long before their `Expiration`.
pub const REFRESH_MARGIN_SECS: i64 = 300;
/// After a failed fetch, or one that returned credentials already inside
/// the refresh margin, wait this long before asking IMDS again. Bounds
/// IMDS traffic (and the lock hold) during an outage, and makes a host
/// without IMDS fail fast between probes.
pub const RETRY_AFTER_SECS: i64 = 30;

/// One set of AWS credentials. `Debug` never prints the secret parts.
#[derive(Clone, PartialEq, Eq)]
pub struct AwsCredentials {
    pub access_key: String,
    pub secret_key: String,
    /// Empty for long-lived keys; set for role credentials.
    pub session_token: String,
}

impl std::fmt::Debug for AwsCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsCredentials")
            .field("access_key", &self.access_key)
            .field("secret_key", &"<redacted>")
            .field(
                "session_token",
                &if self.session_token.is_empty() {
                    ""
                } else {
                    "<redacted>"
                },
            )
            .finish()
    }
}

/// Where a client gets its credentials, decided once at construction.
#[derive(Debug, Clone)]
pub enum CredentialSource {
    /// Keys from the environment.
    Static(AwsCredentials),
    /// The EC2 instance role, through the shared IMDS cache.
    InstanceRole(Arc<InstanceRoleProvider>),
    /// No environment keys and IMDS disabled.
    Unavailable,
    /// Only one of the two static keys is set (boto3
    /// `PartialCredentialsError`); carries the message.
    Partial(String),
}

/// Message for [`CredentialSource::Unavailable`] (kept from the
/// env-only transport so existing log searches still match).
pub const NOT_CONFIGURED: &str =
    "AWS credentials are not configured (AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY).";

impl CredentialSource {
    /// Resolve the source from the process environment, boto3-style:
    /// both static keys set wins; otherwise the instance role unless
    /// `AWS_EC2_METADATA_DISABLED=true`.
    pub fn from_env() -> Self {
        Self::from_vars(|name| std::env::var(name).unwrap_or_default())
    }

    /// [`Self::from_env`] over an explicit variable lookup.
    fn from_vars(var: impl Fn(&str) -> String) -> Self {
        let access_key = var("AWS_ACCESS_KEY_ID");
        let secret_key = var("AWS_SECRET_ACCESS_KEY");
        match (access_key.is_empty(), secret_key.is_empty()) {
            (false, false) => {
                return Self::Static(AwsCredentials {
                    access_key,
                    secret_key,
                    session_token: var("AWS_SESSION_TOKEN"),
                })
            }
            (false, true) => return Self::Partial(partial_message("AWS_SECRET_ACCESS_KEY")),
            (true, false) => return Self::Partial(partial_message("AWS_ACCESS_KEY_ID")),
            (true, true) => {}
        }
        if var("AWS_EC2_METADATA_DISABLED")
            .trim()
            .eq_ignore_ascii_case("true")
        {
            return Self::Unavailable;
        }
        Self::InstanceRole(InstanceRoleProvider::shared())
    }

    /// Fail fast, without a runtime, when the source can never produce
    /// credentials.
    pub fn check_static(&self) -> Result<(), String> {
        match self {
            Self::Static(_) | Self::InstanceRole(_) => Ok(()),
            Self::Unavailable => Err(NOT_CONFIGURED.to_owned()),
            Self::Partial(message) => Err(message.clone()),
        }
    }

    /// Current credentials; role credentials may hit IMDS.
    pub async fn resolve(&self) -> Result<AwsCredentials, String> {
        match self {
            Self::Static(creds) => Ok(creds.clone()),
            Self::InstanceRole(provider) => provider.credentials().await.map_err(|err| {
                format!(
                    "AWS credentials are not configured: AWS_ACCESS_KEY_ID / \
                     AWS_SECRET_ACCESS_KEY are unset and the EC2 instance role \
                     is unavailable ({err})."
                )
            }),
            Self::Unavailable => Err(NOT_CONFIGURED.to_owned()),
            Self::Partial(message) => Err(message.clone()),
        }
    }
}

fn partial_message(missing: &str) -> String {
    format!("Partial AWS credentials found in env, missing: {missing}.")
}

#[derive(Debug, Clone)]
struct CachedCredentials {
    credentials: AwsCredentials,
    expires_at: DateTime<Utc>,
}

/// IMDSv2 instance-role credentials with a refresh-ahead cache. One
/// shared instance serves the whole process ([`InstanceRoleProvider::shared`]);
/// the async mutex also keeps concurrent misses down to one fetch.
pub struct InstanceRoleProvider {
    endpoint: String,
    client: reqwest::Client,
    retry_after: chrono::Duration,
    state: Mutex<ProviderState>,
}

#[derive(Debug, Default, Clone)]
struct ProviderState {
    cached: Option<CachedCredentials>,
    /// No IMDS fetch before this instant (set after a failed fetch or a
    /// fetch that returned near-expiry credentials).
    next_fetch_at: Option<DateTime<Utc>>,
    /// The last fetch error, replayed while `next_fetch_at` holds.
    last_error: Option<String>,
}

impl std::fmt::Debug for InstanceRoleProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstanceRoleProvider")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl InstanceRoleProvider {
    /// A provider against `endpoint` (IMDS base URL, no trailing path).
    ///
    /// The client never uses a proxy (IMDS is link-local and the session
    /// token must not leave the host; `HTTP_PROXY` would otherwise apply)
    /// and keeps no idle connections: fetches are rare, and a pooled
    /// connection is bound to the runtime that opened it, while this
    /// provider is process-wide and may be used from several runtimes.
    pub fn new(endpoint: &str) -> Self {
        let client = reqwest::Client::builder()
            .no_proxy()
            .pool_max_idle_per_host(0)
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            endpoint: endpoint.trim().trim_end_matches('/').to_owned(),
            client,
            retry_after: chrono::Duration::seconds(RETRY_AFTER_SECS),
            state: Mutex::new(ProviderState::default()),
        }
    }

    /// The process-wide provider (endpoint from
    /// `AWS_EC2_METADATA_SERVICE_ENDPOINT`, read once).
    pub fn shared() -> Arc<Self> {
        static SHARED: OnceLock<Arc<InstanceRoleProvider>> = OnceLock::new();
        SHARED
            .get_or_init(|| {
                let configured =
                    std::env::var("AWS_EC2_METADATA_SERVICE_ENDPOINT").unwrap_or_default();
                let endpoint = if configured.trim().is_empty() {
                    DEFAULT_IMDS_ENDPOINT.to_owned()
                } else {
                    configured
                };
                Arc::new(Self::new(&endpoint))
            })
            .clone()
    }

    /// Cached credentials, refetched inside the refresh margin.
    ///
    /// Fetches are rate-limited to one per [`RETRY_AFTER_SECS`] after a
    /// failure or a near-expiry result. In between, unexpired cached
    /// credentials are served, and with none the last error is replayed
    /// without touching IMDS — so an outage costs one slow fetch per
    /// interval rather than one per request, and a host without IMDS fails
    /// fast between probes.
    pub async fn credentials(&self) -> Result<AwsCredentials, String> {
        let mut state = self.state.lock().await;
        let now = Utc::now();
        let margin = chrono::Duration::seconds(REFRESH_MARGIN_SECS);
        let usable = |state: &ProviderState| {
            state
                .cached
                .as_ref()
                .filter(|cached| cached.expires_at > now)
                .map(|cached| cached.credentials.clone())
        };

        if let Some(cached) = state.cached.as_ref() {
            if cached.expires_at - margin > now {
                return Ok(cached.credentials.clone());
            }
        }
        if state.next_fetch_at.is_some_and(|at| now < at) {
            if let Some(credentials) = usable(&state) {
                return Ok(credentials);
            }
            return Err(state
                .last_error
                .clone()
                .unwrap_or_else(|| "IMDS credentials are near expiry".to_owned()));
        }

        match self.fetch().await {
            Ok(fresh) => {
                let credentials = fresh.credentials.clone();
                // IMDS may still hand out the old set inside the margin;
                // don't ask again on every request until it rotates.
                state.next_fetch_at =
                    (fresh.expires_at - margin <= now).then(|| now + self.retry_after);
                state.last_error = None;
                state.cached = Some(fresh);
                Ok(credentials)
            }
            Err(err) => {
                state.next_fetch_at = Some(now + self.retry_after);
                state.last_error = Some(err.clone());
                match usable(&state) {
                    Some(credentials) => {
                        tracing::warn!(error = %err, "aws_credentials.imds_refresh_failed_using_cached");
                        Ok(credentials)
                    }
                    None => Err(err),
                }
            }
        }
    }

    async fn fetch(&self) -> Result<CachedCredentials, String> {
        let token = self
            .client
            .put(format!("{}/latest/api/token", self.endpoint))
            .header("X-aws-ec2-metadata-token-ttl-seconds", IMDS_TOKEN_TTL_SECS)
            .send()
            .await
            .map_err(|err| format!("IMDS token request failed: {err}"))?;
        let token = success_text(token, "IMDS token request").await?;

        let base = format!(
            "{}/latest/meta-data/iam/security-credentials/",
            self.endpoint
        );
        let roles = self
            .client
            .get(&base)
            .header("X-aws-ec2-metadata-token", &token)
            .send()
            .await
            .map_err(|err| format!("IMDS role lookup failed: {err}"))?;
        if roles.status() == reqwest::StatusCode::NOT_FOUND {
            return Err("no IAM role is attached to this instance".to_owned());
        }
        let roles = success_text(roles, "IMDS role lookup").await?;
        let role = roles
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .ok_or_else(|| "no IAM role is attached to this instance".to_owned())?
            .to_owned();

        let body = self
            .client
            .get(format!("{base}{role}"))
            .header("X-aws-ec2-metadata-token", &token)
            .send()
            .await
            .map_err(|err| format!("IMDS credential request failed: {err}"))?;
        let body = success_text(body, "IMDS credential request").await?;
        parse_role_credentials(&body)
    }
}

async fn success_text(response: reqwest::Response, what: &str) -> Result<String, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(format!("{what} returned HTTP {}", status.as_u16()));
    }
    response
        .text()
        .await
        .map_err(|err| format!("{what} body failed: {err}"))
}

/// Parse the IMDS role-credentials document (`Code`, `AccessKeyId`,
/// `SecretAccessKey`, `Token`, `Expiration`).
fn parse_role_credentials(body: &str) -> Result<CachedCredentials, String> {
    let doc: serde_json::Value = body
        .parse()
        .map_err(|err| format!("IMDS credentials are not JSON: {err}"))?;
    let field = |name: &str| doc.get(name).and_then(|v| v.as_str()).unwrap_or_default();
    let code = field("Code");
    if !code.is_empty() && code != "Success" {
        return Err(format!("IMDS credentials are not available (Code={code})"));
    }
    let access_key = field("AccessKeyId").to_owned();
    let secret_key = field("SecretAccessKey").to_owned();
    if access_key.is_empty() || secret_key.is_empty() {
        return Err("IMDS credentials document has no AccessKeyId/SecretAccessKey".to_owned());
    }
    let expires_at = DateTime::parse_from_rfc3339(field("Expiration"))
        .map_err(|err| format!("IMDS credentials have no usable Expiration: {err}"))?
        .with_timezone(&Utc);
    Ok(CachedCredentials {
        credentials: AwsCredentials {
            access_key,
            secret_key,
            session_token: field("Token").to_owned(),
        },
        expires_at,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::extract::{Path, State};
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::{get, put};
    use axum::Router;

    pub(crate) const FAKE_TOKEN: &str = "imds-session-token";
    pub(crate) const FAKE_ROLE: &str = "test-role";

    /// Fake IMDS state: how many credential documents were served, and
    /// how far in the future each one expires.
    #[derive(Clone)]
    pub(crate) struct FakeImds {
        pub fetches: Arc<AtomicUsize>,
        /// IMDSv2 session-token requests (one per fetch attempt).
        pub tokens: Arc<AtomicUsize>,
        pub lifetime_secs: i64,
        pub with_role: bool,
    }

    fn token_ok(headers: &HeaderMap) -> bool {
        headers
            .get("X-aws-ec2-metadata-token")
            .and_then(|v| v.to_str().ok())
            == Some(FAKE_TOKEN)
    }

    /// IMDS routes, mountable next to other fake endpoints.
    pub(crate) fn imds_router(state: FakeImds) -> Router {
        Router::new()
            .route(
                "/latest/api/token",
                put(|State(s): State<FakeImds>, headers: HeaderMap| async move {
                    s.tokens.fetch_add(1, Ordering::SeqCst);
                    if headers.contains_key("X-aws-ec2-metadata-token-ttl-seconds") {
                        (StatusCode::OK, FAKE_TOKEN.to_owned())
                    } else {
                        (StatusCode::BAD_REQUEST, String::new())
                    }
                }),
            )
            .route(
                "/latest/meta-data/iam/security-credentials/",
                get(|State(s): State<FakeImds>, headers: HeaderMap| async move {
                    if !token_ok(&headers) {
                        return (StatusCode::UNAUTHORIZED, String::new());
                    }
                    if !s.with_role {
                        return (StatusCode::NOT_FOUND, String::new());
                    }
                    (StatusCode::OK, format!("{FAKE_ROLE}\n"))
                }),
            )
            .route(
                "/latest/meta-data/iam/security-credentials/{role}",
                get(
                    |State(s): State<FakeImds>, Path(role): Path<String>, headers: HeaderMap| async move {
                        if !token_ok(&headers) || role != FAKE_ROLE {
                            return (StatusCode::UNAUTHORIZED, String::new());
                        }
                        let n = s.fetches.fetch_add(1, Ordering::SeqCst) + 1;
                        let expiration = (Utc::now() + chrono::Duration::seconds(s.lifetime_secs))
                            .format("%Y-%m-%dT%H:%M:%SZ");
                        (
                            StatusCode::OK,
                            serde_json::json!({
                                "Code": "Success",
                                "Type": "AWS-HMAC",
                                "AccessKeyId": format!("ASIAFAKE{n}"),
                                "SecretAccessKey": "fake-secret",
                                "Token": format!("fake-session-{n}"),
                                "Expiration": expiration.to_string(),
                            })
                            .to_string(),
                        )
                    },
                ),
            )
            .with_state(state)
    }

    /// Serve `router` on an ephemeral local port; returns the base URL.
    pub(crate) async fn serve(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn fake(lifetime_secs: i64, with_role: bool) -> FakeImds {
        FakeImds {
            fetches: Arc::new(AtomicUsize::new(0)),
            tokens: Arc::new(AtomicUsize::new(0)),
            lifetime_secs,
            with_role,
        }
    }

    #[tokio::test]
    async fn role_credentials_are_fetched_and_cached() {
        let state = fake(3600, true);
        let base = serve(imds_router(state.clone())).await;
        let provider = InstanceRoleProvider::new(&base);

        let first = provider.credentials().await.expect("role creds");
        assert_eq!(first.access_key, "ASIAFAKE1");
        assert_eq!(first.secret_key, "fake-secret");
        assert_eq!(first.session_token, "fake-session-1");

        let second = provider.credentials().await.expect("cached creds");
        assert_eq!(second, first);
        assert_eq!(state.fetches.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn near_expiry_credentials_are_refetched_at_most_once_per_interval() {
        // Expires in 60 s, inside the 300 s margin.
        let state = fake(60, true);
        let base = serve(imds_router(state.clone())).await;
        let provider = InstanceRoleProvider::new(&base);

        assert_eq!(
            provider.credentials().await.unwrap().access_key,
            "ASIAFAKE1"
        );
        // Inside the retry interval: served from cache, IMDS untouched.
        assert_eq!(
            provider.credentials().await.unwrap().access_key,
            "ASIAFAKE1"
        );
        assert_eq!(state.fetches.load(Ordering::SeqCst), 1);

        // Interval elapsed: refetch.
        provider.state.lock().await.next_fetch_at = None;
        assert_eq!(
            provider.credentials().await.unwrap().access_key,
            "ASIAFAKE2"
        );
        assert_eq!(state.fetches.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn failed_refresh_keeps_unexpired_cached_credentials() {
        let state = fake(60, true);
        let base = serve(imds_router(state.clone())).await;
        let provider = InstanceRoleProvider::new(&base);
        let cached = provider.credentials().await.unwrap();

        // Point the same state at an endpoint that refuses connections,
        // with the retry interval already elapsed.
        let mut seeded = provider.state.lock().await.clone();
        seeded.next_fetch_at = None;
        let broken = InstanceRoleProvider {
            endpoint: "http://127.0.0.1:1".to_owned(),
            client: provider.client.clone(),
            retry_after: provider.retry_after,
            state: Mutex::new(seeded),
        };
        assert_eq!(broken.credentials().await.unwrap(), cached);
        assert!(broken.state.lock().await.next_fetch_at.is_some());
    }

    #[tokio::test]
    async fn failures_are_replayed_without_hitting_imds_until_the_interval() {
        let state = fake(3600, false);
        let base = serve(imds_router(state.clone())).await;
        let provider = InstanceRoleProvider::new(&base);

        let first = provider.credentials().await.expect_err("no role");
        let second = provider.credentials().await.expect_err("replayed");
        assert_eq!(first, second);
        assert_eq!(state.tokens.load(Ordering::SeqCst), 1);

        provider.state.lock().await.next_fetch_at = None;
        let _ = provider.credentials().await.expect_err("probe again");
        let _ = provider.credentials().await.expect_err("replayed again");
        assert_eq!(state.tokens.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn instance_without_role_reports_it() {
        let base = serve(imds_router(fake(3600, false))).await;
        let err = InstanceRoleProvider::new(&base)
            .credentials()
            .await
            .expect_err("no role");
        assert!(err.contains("no IAM role"), "{err}");
    }

    #[tokio::test]
    async fn unreachable_imds_names_the_instance_role_in_the_error() {
        let source = CredentialSource::InstanceRole(Arc::new(InstanceRoleProvider::new(
            "http://127.0.0.1:1",
        )));
        let err = source.resolve().await.expect_err("unreachable");
        assert!(err.contains("EC2 instance role is unavailable"), "{err}");
    }

    #[test]
    fn unconfigured_sources_fail_without_a_runtime() {
        assert_eq!(
            CredentialSource::Unavailable.check_static(),
            Err(NOT_CONFIGURED.to_owned())
        );
        let partial = CredentialSource::Partial(partial_message("AWS_SECRET_ACCESS_KEY"));
        assert!(partial.check_static().is_err());
    }

    fn source_for(vars: &[(&str, &str)]) -> CredentialSource {
        let vars: std::collections::HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        CredentialSource::from_vars(|name| vars.get(name).cloned().unwrap_or_default())
    }

    #[test]
    fn env_source_selection_follows_boto3() {
        assert!(matches!(
            source_for(&[("AWS_ACCESS_KEY_ID", "A"), ("AWS_SECRET_ACCESS_KEY", "S")]),
            CredentialSource::Static(_)
        ));
        match source_for(&[("AWS_ACCESS_KEY_ID", "A")]) {
            CredentialSource::Partial(message) => {
                assert!(
                    message.contains("missing: AWS_SECRET_ACCESS_KEY"),
                    "{message}"
                )
            }
            other => panic!("expected Partial, got {other:?}"),
        }
        match source_for(&[("AWS_SECRET_ACCESS_KEY", "S")]) {
            CredentialSource::Partial(message) => {
                assert!(message.contains("missing: AWS_ACCESS_KEY_ID"), "{message}")
            }
            other => panic!("expected Partial, got {other:?}"),
        }
        assert!(matches!(
            source_for(&[("AWS_EC2_METADATA_DISABLED", "TRUE")]),
            CredentialSource::Unavailable
        ));
        assert!(matches!(source_for(&[]), CredentialSource::InstanceRole(_)));
    }

    #[test]
    fn role_document_parsing() {
        let parsed = parse_role_credentials(
            r#"{"Code":"Success","AccessKeyId":"A","SecretAccessKey":"S","Token":"T","Expiration":"2030-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        assert_eq!(parsed.credentials.session_token, "T");
        assert!(parse_role_credentials(r#"{"Code":"Failure"}"#)
            .unwrap_err()
            .contains("Code=Failure"));
        assert!(parse_role_credentials(
            r#"{"AccessKeyId":"A","SecretAccessKey":"S","Expiration":"soon"}"#
        )
        .unwrap_err()
        .contains("Expiration"));
    }

    #[test]
    fn debug_redacts_secrets() {
        let creds = AwsCredentials {
            access_key: "AKIA".to_owned(),
            secret_key: "very-secret".to_owned(),
            session_token: "tok".to_owned(),
        };
        let shown = format!("{creds:?}");
        assert!(
            !shown.contains("very-secret") && !shown.contains("tok\""),
            "{shown}"
        );
    }
}
