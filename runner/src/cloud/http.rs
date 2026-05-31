// Per-runner HTTPS transport.
//
// See `.ai_design/move_to_https/design.md` and
// `.ai_design/move_to_https/daemon_module.md`.
//
// Three top-level types:
//
// * [`SharedHttpTransport`] — daemon-shared `reqwest::Client` pool.
// * [`RunnerCloudClient`] — one per `RunnerInstance`. Owns that runner's
//   refresh + access tokens, the on-disk credentials handle, and the
//   single-flight refresh gate. All POSTs flow through it.
// * [`HttpLoop`] — one per `RunnerInstance`. Opens a session, polls
//   forever, and dispatches `ForceRefresh` / `Revoke` inline; everything
//   else lands in the runner's mailbox.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, RETRY_AFTER};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use tokio::sync::{Mutex, mpsc, watch};
use tokio::time::sleep;
use uuid::Uuid;

use crate::cloud::protocol::{
    ClientMsg, Envelope, RunnerStatus as WireStatus, ServerMsg, WIRE_VERSION,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("network: {0}")]
    Network(String),
    #[error("auth: access_token_expired")]
    AccessTokenExpired,
    #[error("auth: refresh_token_replayed")]
    RefreshTokenReplayed,
    #[error("auth: membership_revoked")]
    MembershipRevoked,
    #[error("auth: runner_revoked")]
    RunnerRevoked,
    #[error("auth: runner_id_mismatch")]
    RunnerIdMismatch,
    #[error("auth: invalid_refresh_token")]
    InvalidRefreshToken,
    #[error("session_evicted: {reason}")]
    SessionEvicted { reason: String },
    #[error("concurrent_poll")]
    ConcurrentPoll,
    #[error("rate_limited")]
    RateLimited,
    #[error("server: HTTP {status}: {body}")]
    Server { status: u16, body: String },
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("local: {0}")]
    Local(String),
    #[error("local teardown: {0}")]
    LocalTeardown(String),
}

impl TransportError {
    /// True for transport errors that should not unwind a `RunnerInstance`.
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            TransportError::Network(_)
                | TransportError::Server { .. }
                | TransportError::AccessTokenExpired
                | TransportError::RateLimited
        )
    }

    /// True when the server's reply means this runner must shut down.
    /// `SessionEvicted` is intentionally NOT here — eviction just means
    /// "another session opened for this runner_id, yours got closed."
    /// The right response is to reopen and continue, not to give up. The
    /// run loop has a dedicated arm for it.
    pub fn is_fatal_for_runner(&self) -> bool {
        matches!(
            self,
            TransportError::RefreshTokenReplayed
                | TransportError::MembershipRevoked
                | TransportError::RunnerRevoked
                | TransportError::RunnerIdMismatch
                | TransportError::InvalidRefreshToken
                | TransportError::Local(_)
                | TransportError::LocalTeardown(_)
        )
    }

    /// True when a local daemon invariant broke and a process restart is
    /// the right self-healing action.
    pub fn requires_daemon_restart(&self) -> bool {
        matches!(self, TransportError::Local(_))
    }

    /// True when a local runner teardown intentionally closed the mailbox.
    pub fn is_expected_teardown(&self) -> bool {
        matches!(self, TransportError::LocalTeardown(_))
    }
}

// ---------------------------------------------------------------------------
// Shared transport
// ---------------------------------------------------------------------------

/// One per daemon. Cheaply cloned — the inner `Client` keeps its own
/// connection pool internally, so all runners on the daemon share a
/// single HTTP/2 keep-alive arena.
#[derive(Clone)]
pub struct SharedHttpTransport {
    http: Client,
    cloud_url: String,
}

impl SharedHttpTransport {
    pub fn new(cloud_url: String) -> Result<Self> {
        Self::new_with_timeout(cloud_url, Duration::from_secs(60))
    }

    pub fn new_with_timeout(cloud_url: String, timeout: Duration) -> Result<Self> {
        let http = Client::builder()
            .pool_idle_timeout(Duration::from_secs(60))
            .timeout(timeout)
            .user_agent(format!("pidash/{}", crate::RUNNER_VERSION))
            .build()
            .context("building shared reqwest::Client")?;
        Ok(Self {
            http,
            cloud_url: cloud_url.trim_end_matches('/').to_string(),
        })
    }

    pub fn cloud_url(&self) -> &str {
        &self.cloud_url
    }

    pub fn http(&self) -> &Client {
        &self.http
    }
}

// ---------------------------------------------------------------------------
// Per-runner credentials handle
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerCredentials {
    pub runner_id: Uuid,
    pub name: String,
    pub refresh_token: String,
    pub refresh_token_generation: u64,
}

#[derive(Debug, Clone)]
pub struct CredentialsHandle {
    path: PathBuf,
    state: Arc<Mutex<RunnerCredentials>>,
}

impl CredentialsHandle {
    pub fn new(path: PathBuf, creds: RunnerCredentials) -> Self {
        Self {
            path,
            state: Arc::new(Mutex::new(creds)),
        }
    }

    pub async fn snapshot(&self) -> RunnerCredentials {
        self.state.lock().await.clone()
    }

    /// Persist the new refresh token to disk via temp+rename; only
    /// updates the in-memory copy after a successful fsync. Caller is
    /// expected to hold the runner-cloud-client lock so two refreshes
    /// don't race the same file.
    pub async fn rotate(&self, new_token: String, new_generation: u64) -> Result<()> {
        let snapshot = {
            let guard = self.state.lock().await;
            RunnerCredentials {
                runner_id: guard.runner_id,
                name: guard.name.clone(),
                refresh_token: new_token.clone(),
                refresh_token_generation: new_generation,
            }
        };
        let body = toml::to_string_pretty(&CredentialsFile::from(&snapshot))
            .context("serializing credentials")?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| anyhow!("credentials path has no parent"))?;
        tokio::fs::create_dir_all(parent)
            .await
            .context("creating credentials parent dir")?;
        let tmp = self.path.with_extension("toml.tmp");
        tokio::fs::write(&tmp, body.as_bytes())
            .await
            .context("writing credentials tmp file")?;
        // 0600 perms on unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = tokio::fs::metadata(&tmp).await?.permissions();
            perms.set_mode(0o600);
            tokio::fs::set_permissions(&tmp, perms).await?;
        }
        tokio::fs::rename(&tmp, &self.path)
            .await
            .context("renaming credentials tmp file")?;
        let mut guard = self.state.lock().await;
        guard.refresh_token = new_token;
        guard.refresh_token_generation = new_generation;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct CredentialsFile {
    runner: CredentialsRunnerSection,
    refresh: CredentialsRefreshSection,
}

#[derive(Debug, Serialize, Deserialize)]
struct CredentialsRunnerSection {
    id: Uuid,
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CredentialsRefreshSection {
    token: String,
    generation: u64,
}

impl From<&RunnerCredentials> for CredentialsFile {
    fn from(c: &RunnerCredentials) -> Self {
        Self {
            runner: CredentialsRunnerSection {
                id: c.runner_id,
                name: c.name.clone(),
            },
            refresh: CredentialsRefreshSection {
                token: c.refresh_token.clone(),
                generation: c.refresh_token_generation,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Token + session state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AccessToken {
    pub raw: String,
    pub expires_at: DateTime<Utc>,
}

impl AccessToken {
    pub fn is_expired(&self, skew_secs: i64) -> bool {
        let now = Utc::now();
        let cushion = chrono::Duration::seconds(skew_secs);
        self.expires_at <= (now + cushion)
    }
}

#[derive(Debug, Clone)]
pub struct SessionState {
    pub session_id: Uuid,
    pub server_time: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttachBody {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub status: String,
    pub in_flight_run: Option<Uuid>,
    pub project_slug: Option<String>,
    pub host_label: String,
    pub agent_versions: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// RunnerCloudClient — per-runner auth state machine
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct RunnerCloudClient {
    inner: Arc<RunnerCloudClientInner>,
}

struct RunnerCloudClientInner {
    runner_id: Uuid,
    creds: CredentialsHandle,
    transport: SharedHttpTransport,
    state: Mutex<RunnerCloudClientState>,
}

#[derive(Default)]
struct RunnerCloudClientState {
    access_token: Option<AccessToken>,
    session: Option<SessionState>,
    refresh_in_flight: Option<watch::Receiver<RefreshOutcome>>,
}

#[derive(Clone, Debug, Default)]
enum RefreshOutcome {
    #[default]
    Pending,
    Done(Result<(), TransportErrorCode>),
}

#[derive(Clone, Debug)]
struct TransportErrorCode(String);

impl TransportErrorCode {
    fn from(err: &TransportError) -> Self {
        Self(format!("{err}"))
    }
    fn into_err(self) -> TransportError {
        // We round-trip the error tag; specific variants matter for
        // routing, so reconstruct the auth-class ones precisely. For
        // anything else fall back to Network.
        match self.0.as_str() {
            "auth: access_token_expired" => TransportError::AccessTokenExpired,
            "auth: refresh_token_replayed" => TransportError::RefreshTokenReplayed,
            "auth: membership_revoked" => TransportError::MembershipRevoked,
            "auth: runner_revoked" => TransportError::RunnerRevoked,
            "auth: runner_id_mismatch" => TransportError::RunnerIdMismatch,
            "auth: invalid_refresh_token" => TransportError::InvalidRefreshToken,
            other => TransportError::Network(other.to_string()),
        }
    }
}

impl RunnerCloudClient {
    pub fn new(runner_id: Uuid, creds: CredentialsHandle, transport: SharedHttpTransport) -> Self {
        Self {
            inner: Arc::new(RunnerCloudClientInner {
                runner_id,
                creds,
                transport,
                state: Mutex::new(RunnerCloudClientState::default()),
            }),
        }
    }

    pub fn runner_id(&self) -> Uuid {
        self.inner.runner_id
    }

    pub fn transport(&self) -> &SharedHttpTransport {
        &self.inner.transport
    }

    pub async fn access_token_exp(&self) -> Option<DateTime<Utc>> {
        self.inner
            .state
            .lock()
            .await
            .access_token
            .as_ref()
            .map(|t| t.expires_at)
    }

    /// Ensure we have a non-expired access token. Refreshes if the
    /// current one is missing or about to expire.
    pub async fn ensure_access_token(&self) -> Result<AccessToken, TransportError> {
        {
            let guard = self.inner.state.lock().await;
            if let Some(token) = &guard.access_token
                && !token.is_expired(60)
            {
                return Ok(token.clone());
            }
        }
        self.refresh().await?;
        let guard = self.inner.state.lock().await;
        guard.access_token.clone().ok_or(TransportError::Protocol(
            "no access token after refresh".into(),
        ))
    }

    /// Single-flight refresh per `RunnerCloudClient`. Concurrent
    /// callers piggy-back on the in-flight call.
    pub async fn refresh(&self) -> Result<(), TransportError> {
        let waiter = {
            let mut guard = self.inner.state.lock().await;
            if let Some(rx) = &guard.refresh_in_flight {
                Some(rx.clone())
            } else {
                let (tx, rx) = watch::channel(RefreshOutcome::Pending);
                guard.refresh_in_flight = Some(rx);
                drop(guard);
                let result = self.do_refresh().await;
                let outcome = match &result {
                    Ok(()) => RefreshOutcome::Done(Ok(())),
                    Err(err) => RefreshOutcome::Done(Err(TransportErrorCode::from(err))),
                };
                let _ = tx.send(outcome);
                let mut guard = self.inner.state.lock().await;
                guard.refresh_in_flight = None;
                return result;
            }
        };
        if let Some(mut rx) = waiter {
            // Wait until the channel transitions to Done.
            loop {
                if rx.changed().await.is_err() {
                    return Err(TransportError::Network("refresh waiter dropped".into()));
                }
                match rx.borrow_and_update().clone() {
                    RefreshOutcome::Pending => continue,
                    RefreshOutcome::Done(Ok(())) => return Ok(()),
                    RefreshOutcome::Done(Err(code)) => return Err(code.into_err()),
                }
            }
        }
        Ok(())
    }

    async fn do_refresh(&self) -> Result<(), TransportError> {
        let creds = self.inner.creds.snapshot().await;
        let url = format!(
            "{}/api/v1/runner/runners/{}/refresh/",
            self.inner.transport.cloud_url(),
            self.inner.runner_id
        );
        let resp = self
            .inner
            .transport
            .http()
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {}", creds.refresh_token))
            .header(CONTENT_TYPE, "application/json")
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|e| TransportError::Network(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(map_auth_error(status, &body));
        }
        #[derive(Deserialize)]
        struct RefreshResponse {
            refresh_token: String,
            access_token: String,
            access_token_expires_at: String,
            refresh_token_generation: u64,
        }
        let body: RefreshResponse = resp
            .json()
            .await
            .map_err(|e| TransportError::Protocol(format!("refresh body: {e}")))?;
        let exp = parse_iso(&body.access_token_expires_at)?;
        // Persist the new refresh token before we overwrite the access
        // token, so a crash mid-rotation falls back to the new refresh
        // token (which the cloud rotated to "current").
        self.inner
            .creds
            .rotate(body.refresh_token, body.refresh_token_generation)
            .await
            .map_err(|e| TransportError::Network(e.to_string()))?;
        let mut state = self.inner.state.lock().await;
        state.access_token = Some(AccessToken {
            raw: body.access_token,
            expires_at: exp,
        });
        Ok(())
    }

    /// Force-refresh path called from `HttpLoop` when a `ForceRefresh`
    /// server message arrives. Same as `refresh()` for now.
    pub async fn force_refresh_inline(&self) -> Result<(), TransportError> {
        self.refresh().await
    }

    /// Open a session for this runner. Sets the session state on
    /// success; returns the welcome payload + optional resume_ack.
    pub async fn open_session(
        &self,
        body: AttachBody,
    ) -> Result<OpenSessionResponse, TransportError> {
        let token = self.ensure_access_token().await?;
        let url = format!(
            "{}/api/v1/runner/runners/{}/sessions/",
            self.inner.transport.cloud_url(),
            self.inner.runner_id
        );
        let resp = self
            .inner
            .transport
            .http()
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {}", token.raw))
            .header("X-Runner-Protocol-Version", WIRE_VERSION.to_string())
            .json(&body)
            .send()
            .await
            .map_err(|e| TransportError::Network(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(map_session_error(status, &body));
        }
        let parsed: OpenSessionResponse = resp
            .json()
            .await
            .map_err(|e| TransportError::Protocol(format!("session body: {e}")))?;
        let mut state = self.inner.state.lock().await;
        state.session = Some(SessionState {
            session_id: parsed.session_id,
            server_time: parsed.welcome.server_time.unwrap_or_else(Utc::now),
        });
        Ok(parsed)
    }

    pub async fn close_session(&self) -> Result<(), TransportError> {
        let session = {
            let guard = self.inner.state.lock().await;
            match &guard.session {
                Some(s) => s.session_id,
                None => return Ok(()),
            }
        };
        let token = match self.ensure_access_token().await {
            Ok(t) => t,
            Err(_) => return Ok(()),
        };
        let url = format!(
            "{}/api/v1/runner/runners/{}/sessions/{}/",
            self.inner.transport.cloud_url(),
            self.inner.runner_id,
            session
        );
        let _ = self
            .inner
            .transport
            .http()
            .delete(&url)
            .header(AUTHORIZATION, format!("Bearer {}", token.raw))
            .send()
            .await;
        let mut state = self.inner.state.lock().await;
        state.session = None;
        Ok(())
    }

    pub async fn poll(
        &self,
        ack: Vec<String>,
        status: PollStatus,
        long_poll_interval_secs: u64,
    ) -> Result<PollResponse, TransportError> {
        let (session_id, token_raw) = {
            let token = self.ensure_access_token().await?;
            let guard = self.inner.state.lock().await;
            let session = guard
                .session
                .as_ref()
                .ok_or(TransportError::Protocol("no active session".into()))?
                .session_id;
            (session, token.raw)
        };
        let url = format!(
            "{}/api/v1/runner/runners/{}/sessions/{}/poll",
            self.inner.transport.cloud_url(),
            self.inner.runner_id,
            session_id
        );
        let body = serde_json::json!({ "ack": ack, "status": status });
        // Per-request timeout = server's long-poll block + 5s buffer.
        // Without this, the shared `Client::timeout` (60s) fires before
        // the server's block_ms when an operator bumps
        // `LONG_POLL_INTERVAL_SECS` over 55, dropping in-flight assigns
        // and spinning the backoff loop. Override per-request so the
        // shared client default still protects every other RPC.
        let request_timeout = Duration::from_secs(long_poll_interval_secs.saturating_add(5));
        let resp = self
            .inner
            .transport
            .http()
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {token_raw}"))
            .json(&body)
            .timeout(request_timeout)
            .send()
            .await
            .map_err(|e| TransportError::Network(e.to_string()))?;
        let status = resp.status();
        if status == StatusCode::CONFLICT {
            let body = resp.text().await.unwrap_or_default();
            return Err(TransportError::SessionEvicted { reason: body });
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(map_auth_error(status, &body));
        }
        resp.json::<PollResponse>()
            .await
            .map_err(|e| TransportError::Protocol(format!("poll body: {e}")))
    }

    pub async fn dispatch_client_msg(
        &self,
        env: Envelope<ClientMsg>,
    ) -> Result<(), TransportError> {
        let idempotency_key = env.message_id.to_string();
        let msg = env.body;
        // Helper to serialize once and propagate errors instead of
        // silently sending null. A failed serialization indicates a
        // ClientMsg variant the cloud can't decode — not something to
        // bury.
        let to_value = |m: &ClientMsg| -> Result<Json, TransportError> {
            serde_json::to_value(m)
                .map_err(|e| TransportError::Protocol(format!("serialize ClientMsg: {e}")))
        };
        match msg {
            ClientMsg::Hello { .. } | ClientMsg::Heartbeat { .. } | ClientMsg::Bye { .. } => {
                Err(TransportError::Protocol(
                    "Hello/Heartbeat/Bye are not first-class messages on HTTP; \
                     they are folded into session-open/poll/close"
                        .into(),
                ))
            }
            msg @ ClientMsg::Accept { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "accept", body, &idempotency_key)
                    .await
            }
            msg @ ClientMsg::RunStarted { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "started", body, &idempotency_key)
                    .await
            }
            msg @ ClientMsg::RunEvent { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_event(run_id, body, &idempotency_key).await
            }
            msg @ ClientMsg::RunEvents { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_event(run_id, body, &idempotency_key).await
            }
            msg @ ClientMsg::ApprovalRequest { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "approvals", body, &idempotency_key)
                    .await
            }
            msg @ ClientMsg::RunAwaitingReauth { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "awaiting-reauth", body, &idempotency_key)
                    .await
            }
            msg @ ClientMsg::RunCompleted { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "complete", body, &idempotency_key)
                    .await
            }
            msg @ ClientMsg::RunPaused { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "pause", body, &idempotency_key)
                    .await
            }
            msg @ ClientMsg::RunFailed { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "fail", body, &idempotency_key)
                    .await
            }
            msg @ ClientMsg::RunCancelled { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "cancelled", body, &idempotency_key)
                    .await
            }
            msg @ ClientMsg::RunResumed { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "resumed", body, &idempotency_key)
                    .await
            }
            msg @ ClientMsg::ChatStarted {
                chat_session_id, ..
            } => {
                let body = to_value(&msg)?;
                self.post_chat_lifecycle(chat_session_id, "started", body, &idempotency_key)
                    .await
            }
            msg @ ClientMsg::ChatMessageStarted {
                chat_session_id,
                message_id,
                ..
            } => {
                let body = to_value(&msg)?;
                self.post_chat_message_lifecycle(
                    chat_session_id,
                    message_id,
                    "started",
                    body,
                    &idempotency_key,
                )
                .await
            }
            msg @ ClientMsg::ChatEvent {
                chat_session_id, ..
            } => {
                let body = to_value(&msg)?;
                self.post_chat_lifecycle(chat_session_id, "events", body, &idempotency_key)
                    .await
            }
            msg @ ClientMsg::ChatApprovalRequest {
                chat_session_id, ..
            } => {
                let body = to_value(&msg)?;
                self.post_chat_lifecycle(chat_session_id, "approvals", body, &idempotency_key)
                    .await
            }
            msg @ ClientMsg::ChatMessageCompleted {
                chat_session_id,
                message_id,
                ..
            } => {
                let body = to_value(&msg)?;
                self.post_chat_message_lifecycle(
                    chat_session_id,
                    message_id,
                    "complete",
                    body,
                    &idempotency_key,
                )
                .await
            }
            msg @ ClientMsg::ChatFailed {
                chat_session_id, ..
            } => {
                let body = to_value(&msg)?;
                self.post_chat_lifecycle(chat_session_id, "failed", body, &idempotency_key)
                    .await
            }
            msg @ ClientMsg::ChatClosed {
                chat_session_id, ..
            } => {
                let body = to_value(&msg)?;
                self.post_chat_lifecycle(chat_session_id, "closed", body, &idempotency_key)
                    .await
            }
        }
    }

    async fn post_run_lifecycle(
        &self,
        run_id: Uuid,
        verb: &str,
        body: Json,
        idempotency_key: &str,
    ) -> Result<(), TransportError> {
        let url = format!(
            "{}/api/v1/runner/runs/{}/{}/",
            self.inner.transport.cloud_url(),
            run_id,
            verb,
        );
        self.post_authed_with_retry(&url, body, idempotency_key, |_| true)
            .await?;
        Ok(())
    }

    async fn post_run_event(
        &self,
        run_id: Uuid,
        body: Json,
        idempotency_key: &str,
    ) -> Result<(), TransportError> {
        let url = format!(
            "{}/api/v1/runner/runs/{}/events/",
            self.inner.transport.cloud_url(),
            run_id,
        );
        self.post_authed_with_retry(&url, body, idempotency_key, |_| true)
            .await?;
        Ok(())
    }

    async fn post_chat_lifecycle(
        &self,
        chat_session_id: Uuid,
        verb: &str,
        body: Json,
        idempotency_key: &str,
    ) -> Result<(), TransportError> {
        let url = format!(
            "{}/api/v1/runner/chat/sessions/{}/{}/",
            self.inner.transport.cloud_url(),
            chat_session_id,
            verb,
        );
        self.post_authed_with_retry(&url, body, idempotency_key, |_| true)
            .await?;
        Ok(())
    }

    async fn post_chat_message_lifecycle(
        &self,
        chat_session_id: Uuid,
        message_id: Uuid,
        verb: &str,
        body: Json,
        idempotency_key: &str,
    ) -> Result<(), TransportError> {
        let url = format!(
            "{}/api/v1/runner/chat/sessions/{}/messages/{}/{}/",
            self.inner.transport.cloud_url(),
            chat_session_id,
            message_id,
            verb,
        );
        self.post_authed_with_retry(&url, body, idempotency_key, |_| true)
            .await?;
        Ok(())
    }

    /// POST with bounded retry/backoff and one access-token refresh on
    /// `401 access_token_expired`.
    async fn post_authed_with_retry<F: Fn(&Json) -> bool>(
        &self,
        url: &str,
        body: Json,
        idempotency_key: &str,
        _accept: F,
    ) -> Result<Json, TransportError> {
        let mut attempt: u8 = 0;
        let mut refreshed_access_token = false;
        let mut delay = POST_RETRY_INITIAL_DELAY;
        loop {
            let token = match self.ensure_access_token().await {
                Ok(token) => token,
                Err(err) if should_retry_post_error(&err, attempt) => {
                    tracing::warn!(
                        attempt = attempt + 1,
                        max_attempts = POST_RETRY_ATTEMPTS,
                        error = %err,
                        "runner POST auth setup failed; retrying"
                    );
                    sleep(delay).await;
                    attempt = attempt.saturating_add(1);
                    delay = (delay * 2).min(POST_RETRY_MAX_DELAY);
                    continue;
                }
                Err(err) => return Err(err),
            };
            let req = self
                .inner
                .transport
                .http()
                .post(url)
                .header(AUTHORIZATION, format!("Bearer {}", token.raw))
                .header("Idempotency-Key", idempotency_key);
            let resp = match req.json(&body).send().await {
                Ok(resp) => resp,
                Err(err) => {
                    let err = TransportError::Network(err.to_string());
                    if should_retry_post_error(&err, attempt) {
                        tracing::warn!(
                            attempt = attempt + 1,
                            max_attempts = POST_RETRY_ATTEMPTS,
                            error = %err,
                            "runner POST failed; retrying"
                        );
                        sleep(delay).await;
                        attempt = attempt.saturating_add(1);
                        delay = (delay * 2).min(POST_RETRY_MAX_DELAY);
                        continue;
                    }
                    return Err(err);
                }
            };
            let status = resp.status();
            if status.is_success() {
                let v: Json = resp.json().await.unwrap_or(Json::Null);
                return Ok(v);
            }
            if status == StatusCode::UNAUTHORIZED && !refreshed_access_token {
                let txt = resp.text().await.unwrap_or_default();
                if txt.contains("access_token_expired") {
                    if let Err(err) = self.refresh().await {
                        if should_retry_post_error(&err, attempt) {
                            tracing::warn!(
                                attempt = attempt + 1,
                                max_attempts = POST_RETRY_ATTEMPTS,
                                error = %err,
                                "runner POST token refresh failed; retrying"
                            );
                            sleep(delay).await;
                            attempt = attempt.saturating_add(1);
                            delay = (delay * 2).min(POST_RETRY_MAX_DELAY);
                            continue;
                        }
                        return Err(err);
                    }
                    refreshed_access_token = true;
                    continue;
                }
                return Err(map_auth_error(status, &txt));
            }
            let retry_after = retry_after_delay(resp.headers());
            let txt = resp.text().await.unwrap_or_default();
            let err = map_auth_error(status, &txt);
            if should_retry_post_error(&err, attempt) {
                let sleep_for = retry_after.unwrap_or(delay);
                tracing::warn!(
                    attempt = attempt + 1,
                    max_attempts = POST_RETRY_ATTEMPTS,
                    delay_ms = sleep_for.as_millis(),
                    error = %err,
                    "runner POST returned retryable status; retrying"
                );
                sleep(sleep_for).await;
                attempt = attempt.saturating_add(1);
                delay = (delay * 2).min(POST_RETRY_MAX_DELAY);
                continue;
            }
            return Err(err);
        }
    }
}

fn should_retry_post_error(err: &TransportError, attempt: u8) -> bool {
    if attempt >= POST_RETRY_ATTEMPTS {
        return false;
    }
    match err {
        TransportError::Network(_) | TransportError::RateLimited => true,
        TransportError::Server { status, .. } => *status >= 500,
        _ => false,
    }
}

fn retry_after_delay(headers: &HeaderMap) -> Option<Duration> {
    let raw = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = raw.parse::<u64>() {
        return Some(Duration::from_secs(seconds).min(POST_RETRY_MAX_DELAY));
    }
    let retry_at = DateTime::parse_from_rfc2822(raw).ok()?.with_timezone(&Utc);
    let delay = retry_at
        .signed_duration_since(Utc::now())
        .to_std()
        .unwrap_or_default();
    Some(delay.min(POST_RETRY_MAX_DELAY))
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenSessionResponse {
    pub session_id: Uuid,
    pub welcome: WelcomePayload,
    #[serde(default)]
    pub resume_ack: Option<Json>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WelcomePayload {
    #[serde(default)]
    pub server_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub long_poll_interval_secs: Option<u64>,
    #[serde(default)]
    pub protocol_version: Option<u32>,
    /// Optional version advisories pushed from the cloud. See
    /// ``ServerMsg::Welcome`` for semantics.
    #[serde(default)]
    pub latest_runner_version: Option<String>,
    #[serde(default)]
    pub min_runner_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PollStatus {
    pub status: String,
    pub in_flight_run: Option<Uuid>,
    pub ts: DateTime<Utc>,
    // ----- per-active-run observability snapshot -----
    // Optional; only serialised when `agent_observability_v1` is enabled.
    // `observed_run_id` is the only field that may serialise as `null` on
    // the wire — that explicit null tells the cloud "this runner is idle,
    // clear my live-state row's run binding." All other fields are skipped
    // when None so a stale poll never NULLs out a known-good cloud value.
    // See `.ai_design/runner_agent_bridge/design.md` §4.2.
    /// Always serialised when feature is enabled (including null).
    #[serde(
        rename = "observed_run_id",
        default,
        skip_serializing_if = "PollStatusObservabilityFlag::skip"
    )]
    pub observed_run_id_envelope: PollStatusObservabilityFlag,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_subprocess_alive: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approvals_pending: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_count: Option<u32>,
}

/// Wire-side wrapper used to express the three states of `observed_run_id`:
/// absent (feature off), present-and-null (feature on, runner idle), and
/// present-with-value (feature on, runner busy). Default is `Absent`,
/// which `skip_serializing_if` drops from the JSON entirely.
#[derive(Debug, Clone, Default)]
pub enum PollStatusObservabilityFlag {
    #[default]
    Absent,
    Present(Option<Uuid>),
}

impl PollStatusObservabilityFlag {
    pub fn skip(&self) -> bool {
        matches!(self, Self::Absent)
    }
}

impl Serialize for PollStatusObservabilityFlag {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            // The struct field carries `skip_serializing_if = "skip"`,
            // which intercepts `Absent` before serde reaches this impl.
            // If a future refactor drops that attribute, this branch
            // would silently start emitting `null` — a wire change that
            // looks like "feature on, idle" to the cloud, with no
            // compile-time warning. Treat reaching it as a bug rather
            // than papering over it.
            Self::Absent => Err(serde::ser::Error::custom(
                "PollStatusObservabilityFlag::Absent reached Serialize; \
                 skip_serializing_if was bypassed",
            )),
            Self::Present(v) => match v {
                Some(uuid) => serializer.serialize_some(uuid),
                None => serializer.serialize_none(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub total: u64,
}

impl From<crate::daemon::observability::TokenUsage> for TokenUsage {
    fn from(t: crate::daemon::observability::TokenUsage) -> Self {
        Self {
            input: t.input,
            output: t.output,
            total: t.total,
        }
    }
}

impl PollStatus {
    pub fn idle() -> Self {
        Self {
            status: "idle".to_string(),
            in_flight_run: None,
            ts: Utc::now(),
            observed_run_id_envelope: PollStatusObservabilityFlag::Absent,
            last_event_at: None,
            last_event_kind: None,
            last_event_summary: None,
            agent_pid: None,
            agent_subprocess_alive: None,
            approvals_pending: None,
            tokens: None,
            model: None,
            turn_count: None,
        }
    }

    pub fn from_wire(status: WireStatus, in_flight_run: Option<Uuid>) -> Self {
        let s = match status {
            WireStatus::Idle => "idle",
            WireStatus::Busy => "busy",
            WireStatus::Reconnecting => "reconnecting",
            WireStatus::AwaitingReauth => "awaiting_reauth",
        };
        Self {
            status: s.to_string(),
            in_flight_run,
            ts: Utc::now(),
            observed_run_id_envelope: PollStatusObservabilityFlag::Absent,
            last_event_at: None,
            last_event_kind: None,
            last_event_summary: None,
            agent_pid: None,
            agent_subprocess_alive: None,
            approvals_pending: None,
            tokens: None,
            model: None,
            turn_count: None,
        }
    }

    /// Build a poll status that includes the per-active-run observability
    /// snapshot when the `agent_observability_v1` flag is enabled. When
    /// disabled, this is identical to `from_wire`.
    pub fn from_state(
        status: WireStatus,
        in_flight_run: Option<Uuid>,
        feature_enabled: bool,
        snapshot: crate::daemon::state::ObservabilitySnapshot,
        approvals_pending: usize,
    ) -> Self {
        let mut me = Self::from_wire(status, in_flight_run);
        if !feature_enabled {
            return me;
        }
        // observed_run_id is *always* serialised when the feature is on,
        // including as null when idle. The cloud uses an explicit null to
        // clear the live-state row's run binding (design §4.5.2).
        me.observed_run_id_envelope = PollStatusObservabilityFlag::Present(in_flight_run);
        me.last_event_at = snapshot.last_event_at;
        me.last_event_kind = snapshot.last_event_kind;
        me.last_event_summary = snapshot.last_event_summary;
        me.agent_pid = snapshot.agent_pid;
        me.agent_subprocess_alive = snapshot.agent_subprocess_alive;
        me.approvals_pending = Some(u32::try_from(approvals_pending).unwrap_or(u32::MAX));
        me.tokens = snapshot.tokens.map(TokenUsage::from);
        me.model = snapshot.model;
        me.turn_count = snapshot.turn_count;
        me
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PollResponse {
    pub messages: Vec<PollMessage>,
    #[serde(default)]
    pub server_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub long_poll_interval_secs: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PollMessage {
    pub stream_id: String,
    #[serde(default)]
    pub mid: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    pub body: Json,
}

// ---------------------------------------------------------------------------
// HttpLoop — per-runner long-poll task
// ---------------------------------------------------------------------------

pub struct HttpLoop {
    pub client: RunnerCloudClient,
    pub mailbox: mpsc::Sender<InboundEnvelope>,
    pub ack_rx: mpsc::UnboundedReceiver<AckEntry>,
    pub status_rx: watch::Receiver<WireStatus>,
    pub in_flight_rx: watch::Receiver<Option<Uuid>>,
    pub shutdown: Arc<tokio::sync::Notify>,
    pub attach_body: AttachBody,
    /// Optional handle to the daemon's state. When `Some`, `poll_once`
    /// builds a `PollStatus::from_state` (carrying the per-active-run
    /// observability snapshot). When `None`, falls back to `from_wire`,
    /// which is identical to the v3 wire shape — used by tests and any
    /// caller that doesn't want to thread state through.
    pub state: Option<crate::daemon::state::StateHandle>,
    teardown_rx: Option<watch::Receiver<bool>>,
    inline_acks: VecDeque<String>,
    /// Bounded mid-dedupe (design.md §8 / Decision 21). At-least-once
    /// delivery + the PEL-replay path (`use_zero=True` on the first
    /// poll after session-open) means an `assign` / `cancel` / `decide`
    /// frame can legitimately arrive twice. Without dedupe the runner
    /// would re-execute side-effects each time, converting at-least-
    /// once delivery into at-least-once execution.
    mid_dedupe: MidDedupe,
    /// Per-poll request timeout (server's long-poll block + buffer),
    /// captured from the welcome envelope. Used to override the shared
    /// reqwest client's 60s default so an operator-bumped
    /// `LONG_POLL_INTERVAL_SECS` doesn't cause the client timeout to
    /// fire before the server's block_ms.
    long_poll_interval_secs: u64,
}

/// Maximum number of recently-seen `mid` values retained for daemon-side
/// inbound-frame dedupe. Sized so a multi-hour PEL replay storm fits
/// without churning the ring; ~36-byte UUID strings × 4096 ≈ 150 KiB
/// per running runner instance.
const MID_DEDUPE_CAPACITY: usize = 4096;

/// Hard upper bound on `long_poll_interval_secs` the daemon honors.
/// Server-side `LONG_POLL_INTERVAL_SECS` should also clamp to this; if
/// a misconfigured cloud sends a larger value the daemon caps it
/// locally so a single welcome can't lock us into a multi-minute
/// per-poll wait that masks real failures.
const MAX_LONG_POLL_INTERVAL_SECS: u64 = 55;
/// Fallback when welcome carries no value or the cloud cannot be
/// trusted. Matches the server's documented default.
const DEFAULT_LONG_POLL_INTERVAL_SECS: u64 = 25;
const POST_RETRY_ATTEMPTS: u8 = 8;
const POST_RETRY_INITIAL_DELAY: Duration = Duration::from_millis(500);
const POST_RETRY_MAX_DELAY: Duration = Duration::from_secs(30);

/// Bounded "have we seen this mid before?" set. Cap-bounded ring of
/// recent mids; when the ring is full, the oldest entry is evicted
/// from both the ring and the lookup set so memory stays predictable.
#[derive(Debug)]
struct MidDedupe {
    ring: VecDeque<String>,
    lookup: std::collections::HashSet<String>,
    capacity: usize,
}

impl MidDedupe {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            ring: VecDeque::with_capacity(capacity),
            lookup: std::collections::HashSet::with_capacity(capacity),
            capacity,
        }
    }

    /// Returns `true` if `mid` was already in the ring (caller should
    /// drop the duplicate). Records `mid` if not. Empty mids are not
    /// recorded — we cannot dedupe what we cannot key on.
    fn record(&mut self, mid: &str) -> bool {
        if mid.is_empty() {
            return false;
        }
        if self.lookup.contains(mid) {
            return true;
        }
        if self.ring.len() >= self.capacity
            && let Some(evicted) = self.ring.pop_front()
        {
            self.lookup.remove(&evicted);
        }
        self.ring.push_back(mid.to_string());
        self.lookup.insert(mid.to_string());
        false
    }
}

#[derive(Debug, Clone)]
pub struct AckEntry {
    pub stream_id: String,
}

#[derive(Debug, Clone)]
pub struct InboundEnvelope {
    pub stream_id: Option<String>,
    pub env: Envelope<ServerMsg>,
}

/// Pure helper used by `HttpLoop::current_attach_body` so the refresh
/// logic is testable without standing up a full `RunnerCloudClient`.
fn refresh_attach_body(
    base: &AttachBody,
    status: WireStatus,
    in_flight: Option<Uuid>,
) -> AttachBody {
    let mut body = base.clone();
    body.status = PollStatus::from_wire(status, in_flight).status;
    body.in_flight_run = in_flight;
    body
}

impl HttpLoop {
    pub fn new(
        client: RunnerCloudClient,
        mailbox: mpsc::Sender<InboundEnvelope>,
        ack_rx: mpsc::UnboundedReceiver<AckEntry>,
        status_rx: watch::Receiver<WireStatus>,
        in_flight_rx: watch::Receiver<Option<Uuid>>,
        shutdown: Arc<tokio::sync::Notify>,
        attach_body: AttachBody,
    ) -> Self {
        Self {
            client,
            mailbox,
            ack_rx,
            status_rx,
            in_flight_rx,
            shutdown,
            attach_body,
            state: None,
            teardown_rx: None,
            inline_acks: VecDeque::new(),
            mid_dedupe: MidDedupe::with_capacity(MID_DEDUPE_CAPACITY),
            long_poll_interval_secs: DEFAULT_LONG_POLL_INTERVAL_SECS,
        }
    }

    /// Attach the daemon's state so `poll_once` can include the
    /// per-active-run observability snapshot when the
    /// `agent_observability_v1` flag is enabled.
    pub fn with_state(mut self, state: crate::daemon::state::StateHandle) -> Self {
        self.state = Some(state);
        self
    }

    /// Attach the per-runner teardown latch. When the runner is deliberately
    /// removed, its mailbox closes; treat that as expected teardown rather
    /// than a daemon-level invariant failure.
    pub fn with_teardown_rx(mut self, teardown_rx: watch::Receiver<bool>) -> Self {
        self.teardown_rx = Some(teardown_rx);
        self
    }

    fn mailbox_closed_error(&self, err: impl std::fmt::Display) -> TransportError {
        if self.teardown_rx.as_ref().is_some_and(|rx| *rx.borrow()) {
            TransportError::LocalTeardown(format!("runner mailbox closed after teardown: {err}"))
        } else {
            TransportError::Local(format!("runner mailbox closed: {err}"))
        }
    }

    /// Build a fresh `AttachBody` snapshotting the current status and
    /// in-flight run from the watch receivers. The base body (version,
    /// os/arch, project_slug, host_label, agent_versions) is cloned from
    /// `self.attach_body`; the volatile `status` + `in_flight_run`
    /// fields are refreshed on every call.
    ///
    /// Without this refresh, session reconnects re-send the snapshot
    /// captured at daemon startup — when `in_flight_run` was None — and
    /// the cloud's heartbeat reaper kills any run that was assigned
    /// after startup but before the reconnect (`session_service.py`
    /// `reap_stale_busy_runs`).
    fn current_attach_body(&self) -> AttachBody {
        refresh_attach_body(
            &self.attach_body,
            *self.status_rx.borrow(),
            *self.in_flight_rx.borrow(),
        )
    }

    /// Run `ensure_access_token` then `open_session`, retrying transient
    /// failures with exponential backoff (1s → 30s capped). Returns when
    /// the session is open OR the loop is shutdown. Only fatal-for-runner
    /// errors propagate — recoverable / SessionEvicted errors loop until
    /// success.
    async fn bootstrap_with_retry(&self) -> Result<OpenSessionResponse, TransportError> {
        let mut backoff_secs = 1u64;
        loop {
            let attempt = async {
                self.client.ensure_access_token().await?;
                self.client.open_session(self.current_attach_body()).await
            };
            let shutdown = self.shutdown.clone();
            match tokio::select! {
                _ = shutdown.notified() => {
                    return Err(TransportError::Network("shutdown during bootstrap".into()));
                }
                result = attempt => result,
            } {
                Ok(session) => return Ok(session),
                Err(err) if err.is_fatal_for_runner() => {
                    tracing::warn!(
                        runner = %self.client.runner_id(),
                        "fatal error during bootstrap: {err}",
                    );
                    return Err(err);
                }
                Err(err) => {
                    tracing::warn!(
                        runner = %self.client.runner_id(),
                        backoff_secs,
                        "bootstrap failed; retrying: {err}",
                    );
                    sleep(Duration::from_secs(backoff_secs)).await;
                    backoff_secs = (backoff_secs * 2).min(30);
                }
            }
        }
    }

    pub async fn run(mut self) -> Result<(), TransportError> {
        // 1. Bootstrap: ensure access token, open session. Both calls
        // can hit transient network blips (request timeout, connection
        // reset, brief 5xx during cloud restart) — retry with backoff
        // so a single startup glitch doesn't kill the loop forever. Only
        // genuinely fatal errors (revoked creds, runner_id mismatch)
        // unwind here.
        let session = self.bootstrap_with_retry().await?;
        // Capture the server's long-poll interval (clamped) so the per-
        // request timeout in `RunnerCloudClient::poll` matches the
        // server's actual block deadline.
        self.long_poll_interval_secs = session
            .welcome
            .long_poll_interval_secs
            .unwrap_or(DEFAULT_LONG_POLL_INTERVAL_SECS)
            .min(MAX_LONG_POLL_INTERVAL_SECS);
        // 2. Hand welcome (and optional resume_ack) to the mailbox so
        //    the existing per-runner handlers ingest them as today.
        let welcome_body = ServerMsg::Welcome {
            server_time: session.welcome.server_time.unwrap_or_else(Utc::now),
            heartbeat_interval_secs: self.long_poll_interval_secs,
            protocol_version: session.welcome.protocol_version.unwrap_or(WIRE_VERSION),
            latest_runner_version: session.welcome.latest_runner_version.clone(),
            min_runner_version: session.welcome.min_runner_version.clone(),
        };
        self.mailbox
            .send(InboundEnvelope {
                stream_id: None,
                env: Envelope::for_runner(self.client.runner_id(), welcome_body),
            })
            .await
            .map_err(|e| self.mailbox_closed_error(e))?;
        if let Some(resume_body) = session.resume_ack
            && let Ok(parsed) = serde_json::from_value::<ServerMsg>(resume_body.clone())
        {
            self.mailbox
                .send(InboundEnvelope {
                    stream_id: None,
                    env: Envelope::for_runner(self.client.runner_id(), parsed),
                })
                .await
                .map_err(|e| self.mailbox_closed_error(e))?;
        }

        let mut backoff_secs = 1u64;
        let mut consecutive_evictions = 0u32;
        loop {
            let shutdown = self.shutdown.clone();
            match tokio::select! {
                _ = shutdown.notified() => break,
                result = self.poll_once() => result,
            } {
                Ok(()) => {
                    backoff_secs = 1;
                    consecutive_evictions = 0;
                }
                Err(err) if err.is_fatal_for_runner() => {
                    tracing::warn!(runner = %self.client.runner_id(), "fatal transport error: {err}");
                    return Err(err);
                }
                Err(TransportError::SessionEvicted { reason }) => {
                    // Another session opened for this runner_id and the
                    // cloud closed ours. Reopen and continue. Back off
                    // exponentially on repeats so a competing daemon
                    // doesn't pin us in a thrash loop.
                    consecutive_evictions = consecutive_evictions.saturating_add(1);
                    let backoff =
                        Duration::from_secs((1u64 << consecutive_evictions.min(5)).min(30));
                    tracing::warn!(
                        runner = %self.client.runner_id(),
                        consecutive = consecutive_evictions,
                        backoff_secs = backoff.as_secs(),
                        "session evicted ({reason}); reopening",
                    );
                    sleep(backoff).await;
                    if let Err(e) = self.client.open_session(self.current_attach_body()).await {
                        tracing::warn!(
                            runner = %self.client.runner_id(),
                            "reopen after eviction failed: {e}",
                        );
                    }
                }
                Err(err) if err.is_recoverable() => {
                    // Transient: long-poll timeout, transient 5xx, token
                    // refresh window, etc. Back off and retry on the
                    // EXISTING session — do NOT reopen, because each
                    // reopen evicts our own prior session and a long-poll
                    // already in flight against it would surface as
                    // SessionEvicted on the next loop iteration.
                    tracing::debug!(runner = %self.client.runner_id(), "recoverable transport error: {err}");
                    sleep(Duration::from_secs(backoff_secs)).await;
                    backoff_secs = (backoff_secs * 2).min(30);
                }
                Err(err) => {
                    tracing::error!(runner = %self.client.runner_id(), "transport error: {err}");
                    sleep(Duration::from_secs(backoff_secs)).await;
                    backoff_secs = (backoff_secs * 2).min(30);
                }
            }
        }
        let _ = self.client.close_session().await;
        Ok(())
    }

    async fn poll_once(&mut self) -> Result<(), TransportError> {
        let mut acks = Vec::new();
        while let Some(stream_id) = self.inline_acks.pop_front() {
            acks.push(stream_id);
        }
        while let Ok(entry) = self.ack_rx.try_recv() {
            acks.push(entry.stream_id);
        }
        let wire_status = *self.status_rx.borrow();
        let in_flight = *self.in_flight_rx.borrow();
        let status = match self.state.as_ref() {
            Some(state) if state.agent_observability_v1() => {
                let snapshot = state.observability_snapshot().await;
                let approvals = state.approvals_pending_value().await;
                PollStatus::from_state(wire_status, in_flight, true, snapshot, approvals)
            }
            _ => PollStatus::from_wire(wire_status, in_flight),
        };
        let resp = self
            .client
            .poll(acks, status, self.long_poll_interval_secs)
            .await?;
        for msg in resp.messages {
            self.dispatch(msg).await?;
        }
        Ok(())
    }

    async fn dispatch(&mut self, msg: PollMessage) -> Result<(), TransportError> {
        let stream_id = msg.stream_id.clone();
        let runner_id = self.client.runner_id();
        // At-least-once delivery (design.md §8): a frame the daemon
        // already executed can reappear via PEL replay or stream
        // retries. Drop duplicates by `mid` before they reach the
        // mailbox, but still inline-ack the stream entry so the
        // cloud's outbox doesn't grow unbounded.
        if self.mid_dedupe.record(&msg.mid) {
            tracing::debug!(
                runner = %runner_id,
                mid = %msg.mid,
                kind = ?msg.kind,
                "dropping duplicate inbound frame"
            );
            self.inline_acks.push_back(stream_id);
            return Ok(());
        }
        // Decode the body's "type" field via the ServerMsg enum.
        let parsed: Result<ServerMsg, _> = serde_json::from_value(msg.body.clone());
        match parsed {
            Ok(ServerMsg::ForceRefresh { .. }) => {
                if let Err(e) = self.client.force_refresh_inline().await {
                    tracing::error!(runner = %runner_id, "force_refresh failed: {e}");
                }
                self.inline_acks.push_back(stream_id);
            }
            Ok(parsed_body) => {
                let env = Envelope::<ServerMsg> {
                    version: WIRE_VERSION,
                    message_id: parse_uuid(&msg.mid).unwrap_or_else(Uuid::new_v4),
                    runner_id: Some(runner_id),
                    body: parsed_body,
                };
                if let Err(e) = self
                    .mailbox
                    .send(InboundEnvelope {
                        stream_id: Some(stream_id),
                        env,
                    })
                    .await
                {
                    tracing::warn!(runner = %runner_id, "mailbox send failed: {e}");
                    return Err(self.mailbox_closed_error(e));
                }
            }
            Err(err) => {
                tracing::warn!(
                    runner = %runner_id,
                    "failed to parse server message {kind:?}: {err}",
                    kind = msg.kind,
                );
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_iso(raw: &str) -> Result<DateTime<Utc>, TransportError> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| TransportError::Protocol(format!("parse iso8601 {raw:?}: {e}")))
}

fn parse_uuid(raw: &str) -> Option<Uuid> {
    Uuid::parse_str(raw).ok()
}

fn map_auth_error(status: StatusCode, body: &str) -> TransportError {
    if status == StatusCode::UNAUTHORIZED {
        if body.contains("access_token_expired") {
            return TransportError::AccessTokenExpired;
        }
        if body.contains("refresh_token_replayed") {
            return TransportError::RefreshTokenReplayed;
        }
        if body.contains("membership_revoked") {
            return TransportError::MembershipRevoked;
        }
        if body.contains("runner_revoked") {
            return TransportError::RunnerRevoked;
        }
        if body.contains("invalid_refresh_token") {
            return TransportError::InvalidRefreshToken;
        }
    }
    if status == StatusCode::FORBIDDEN && body.contains("runner_id_mismatch") {
        return TransportError::RunnerIdMismatch;
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return TransportError::RateLimited;
    }
    TransportError::Server {
        status: status.as_u16(),
        body: body.chars().take(256).collect(),
    }
}

fn map_session_error(status: StatusCode, body: &str) -> TransportError {
    if status == StatusCode::CONFLICT {
        return TransportError::SessionEvicted {
            reason: body.chars().take(256).collect(),
        };
    }
    map_auth_error(status, body)
}

/// Inputs for CLI-initiated runner creation.
pub struct CreateRunnerRequest<'a> {
    pub api_token: &'a str,
    pub dev_machine_id: &'a Uuid,
    pub workspace_slug: Option<&'a str>,
    pub project: &'a str,
    pub host_label: &'a str,
    pub name: Option<&'a str>,
    pub pod: Option<&'a str>,
}

/// Public helper to create a runner with the user-scoped CLI `APIToken`.
///
/// `pidash auth login` populates `[cli].token`. Once that token exists,
/// `pidash runner add` can mint a runner directly without the one-time
/// enrollment-token paste. `workspace_slug` is optional: the cloud falls back
/// to the caller's single workspace membership when omitted.
pub async fn create_runner(
    transport: &SharedHttpTransport,
    req: CreateRunnerRequest<'_>,
) -> Result<EnrollResponse, TransportError> {
    let url = format!("{}/api/v1/runner/runners/", transport.cloud_url());
    let mut body = serde_json::json!({
        "project": req.project,
        "dev_machine_id": req.dev_machine_id,
        "host_label": req.host_label,
    });
    if let Some(ws) = req.workspace_slug {
        body["workspace_slug"] = Json::String(ws.to_string());
    }
    if let Some(n) = req.name {
        body["name"] = Json::String(n.to_string());
    }
    if let Some(p) = req.pod {
        body["pod"] = Json::String(p.to_string());
    }
    let resp = transport
        .http()
        .post(&url)
        .header("X-Api-Key", req.api_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| TransportError::Network(e.to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(map_auth_error(status, &body));
    }
    resp.json::<EnrollResponse>()
        .await
        .map_err(|e| TransportError::Protocol(format!("create_runner body: {e}")))
}

pub async fn enroll_runner(
    transport: &SharedHttpTransport,
    enrollment_token: &str,
    host_label: &str,
    name: Option<&str>,
) -> Result<EnrollResponse, TransportError> {
    let url = format!("{}/api/v1/runner/runners/enroll/", transport.cloud_url());
    let mut body = serde_json::json!({
        "enrollment_token": enrollment_token,
        "host_label": host_label,
    });
    if let Some(n) = name {
        body["name"] = Json::String(n.to_string());
    }
    let resp = transport
        .http()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| TransportError::Network(e.to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(map_auth_error(status, &body));
    }
    resp.json::<EnrollResponse>()
        .await
        .map_err(|e| TransportError::Protocol(format!("enroll body: {e}")))
}

/// Read a per-runner `credentials.toml` from disk and return a
/// `CredentialsHandle` ready for use with `RunnerCloudClient`. Centralised
/// here so the supervisor and the CLI/TUI remove flow share one parser.
pub async fn load_runner_credentials_from(
    path: PathBuf,
    runner_name: &str,
) -> Result<CredentialsHandle, TransportError> {
    let raw = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| TransportError::Network(format!("reading {path:?}: {e}")))?;
    let parsed: toml::Value = toml::from_str(&raw)
        .map_err(|e| TransportError::Protocol(format!("parsing {path:?}: {e}")))?;
    let runner_id = parsed
        .get("runner")
        .and_then(|v| v.get("id"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| TransportError::Protocol(format!("{path:?} missing runner.id")))?;
    let refresh_token = parsed
        .get("refresh")
        .and_then(|v| v.get("token"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| TransportError::Protocol(format!("{path:?} missing refresh.token")))?;
    let generation = parsed
        .get("refresh")
        .and_then(|v| v.get("generation"))
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| TransportError::Protocol(format!("{path:?} missing refresh.generation")))?;
    let runner_id = Uuid::parse_str(runner_id)
        .map_err(|e| TransportError::Protocol(format!("invalid runner.id in {path:?}: {e}")))?;
    Ok(CredentialsHandle::new(
        path,
        RunnerCredentials {
            runner_id,
            name: runner_name.to_string(),
            refresh_token: refresh_token.to_string(),
            refresh_token_generation: generation as u64,
        },
    ))
}

pub async fn write_runner_credentials(
    path: PathBuf,
    creds: RunnerCredentials,
) -> Result<(), TransportError> {
    let handle = CredentialsHandle::new(path, creds.clone());
    handle
        .rotate(creds.refresh_token.clone(), creds.refresh_token_generation)
        .await
        .map_err(|e| TransportError::Network(e.to_string()))
}

/// Self-revoke a runner cloud-side via the machine-token auth surface.
/// `DELETE /api/v1/runner/runners/<rid>/`. Idempotent on the server:
/// re-issuing against an already-deleted runner returns 401 (the bearer
/// no longer resolves), which we map back to `Ok(())` so callers don't
/// need to special-case the second invocation. Caller is responsible
/// for cleaning up local state afterwards.
pub async fn revoke_runner_self(client: &RunnerCloudClient) -> Result<(), TransportError> {
    let token = client.ensure_access_token().await?;
    let url = format!(
        "{}/api/v1/runner/runners/{}/",
        client.transport().cloud_url(),
        client.runner_id(),
    );
    let resp = client
        .transport()
        .http()
        .delete(&url)
        .header("Authorization", format!("Bearer {}", token.raw))
        .send()
        .await
        .map_err(|e| TransportError::Network(e.to_string()))?;
    let status = resp.status();
    if status.is_success() || status == StatusCode::NOT_FOUND || status == StatusCode::UNAUTHORIZED
    {
        return Ok(());
    }
    let body = resp.text().await.unwrap_or_default();
    Err(map_auth_error(status, &body))
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnrollResponse {
    pub runner_id: Uuid,
    pub runner_name: String,
    pub refresh_token: String,
    pub access_token: String,
    pub access_token_expires_at: String,
    pub refresh_token_generation: u64,
    pub workspace_slug: String,
    #[serde(default)]
    pub pod_slug: String,
    #[serde(default)]
    pub project_identifier: String,
    pub long_poll_interval_secs: u64,
    pub protocol_version: u32,
    #[serde(default)]
    pub machine_token: Option<String>,
    #[serde(default)]
    pub machine_token_minted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_token_expiry_logic() {
        let token = AccessToken {
            raw: "x".into(),
            expires_at: Utc::now() + chrono::Duration::seconds(120),
        };
        assert!(!token.is_expired(60));
        assert!(token.is_expired(121));
    }

    #[test]
    fn auth_error_mapping() {
        let err = map_auth_error(StatusCode::UNAUTHORIZED, "{\"error\":\"runner_revoked\"}");
        assert!(matches!(err, TransportError::RunnerRevoked));
        let err = map_auth_error(StatusCode::FORBIDDEN, "{\"error\":\"runner_id_mismatch\"}");
        assert!(matches!(err, TransportError::RunnerIdMismatch));
    }

    #[test]
    fn fatal_classification() {
        assert!(TransportError::RunnerRevoked.is_fatal_for_runner());
        assert!(TransportError::RefreshTokenReplayed.is_fatal_for_runner());
        let teardown = TransportError::LocalTeardown("runner removed".into());
        assert!(teardown.is_fatal_for_runner());
        assert!(teardown.is_expected_teardown());
        assert!(!teardown.requires_daemon_restart());
        // SessionEvicted is recoverable — the run loop has a dedicated
        // arm that reopens the session. See `HttpLoop::run`.
        assert!(!TransportError::SessionEvicted { reason: "x".into() }.is_fatal_for_runner());
        assert!(!TransportError::Network("net".into()).is_fatal_for_runner());
    }

    #[test]
    fn retry_after_delay_parses_and_caps_delta_seconds() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, "7".parse().unwrap());
        assert_eq!(retry_after_delay(&headers), Some(Duration::from_secs(7)));

        headers.insert(RETRY_AFTER, "9999".parse().unwrap());
        assert_eq!(retry_after_delay(&headers), Some(POST_RETRY_MAX_DELAY));
    }

    #[test]
    fn post_retry_classification_is_limited_to_transient_failures() {
        assert!(should_retry_post_error(
            &TransportError::Network("timeout".into()),
            0
        ));
        assert!(should_retry_post_error(&TransportError::RateLimited, 0));
        assert!(should_retry_post_error(
            &TransportError::Server {
                status: 503,
                body: "unavailable".into()
            },
            0
        ));
        assert!(!should_retry_post_error(
            &TransportError::Server {
                status: 400,
                body: "bad request".into()
            },
            0
        ));
        assert!(!should_retry_post_error(&TransportError::RunnerRevoked, 0));
        assert!(!should_retry_post_error(
            &TransportError::Network("timeout".into()),
            POST_RETRY_ATTEMPTS
        ));
    }

    fn sample_attach_body() -> AttachBody {
        AttachBody {
            version: "0.0.0".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            status: PollStatus::idle().status,
            in_flight_run: None,
            project_slug: Some("p".into()),
            host_label: "h".into(),
            agent_versions: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn refresh_attach_body_picks_up_live_in_flight_run() {
        // The bug we're guarding: at startup `attach_body.in_flight_run`
        // is None; after a run is assigned, the watch channel carries
        // Some(rid). A reconnect must reflect the live value or the
        // cloud reaper kills the active run.
        let base = sample_attach_body();
        let rid = Uuid::new_v4();
        let refreshed = refresh_attach_body(&base, WireStatus::Busy, Some(rid));
        assert_eq!(refreshed.in_flight_run, Some(rid));
        // Status follows in_flight: non-idle when a run is in progress.
        assert_ne!(refreshed.status, PollStatus::idle().status);
        // Non-volatile fields are preserved from the base.
        assert_eq!(refreshed.version, "0.0.0");
        assert_eq!(refreshed.host_label, "h");
        assert_eq!(refreshed.project_slug.as_deref(), Some("p"));
    }

    #[test]
    fn refresh_attach_body_clears_in_flight_when_idle() {
        // After a run completes, in_flight returns to None and the next
        // reconnect should report Idle to the cloud.
        let mut base = sample_attach_body();
        base.in_flight_run = Some(Uuid::new_v4());
        base.status = PollStatus::from_wire(WireStatus::Busy, base.in_flight_run).status;
        let refreshed = refresh_attach_body(&base, WireStatus::Idle, None);
        assert_eq!(refreshed.in_flight_run, None);
        assert_eq!(refreshed.status, PollStatus::idle().status);
    }

    #[test]
    fn attach_body_serialization_unchanged_by_observability_design() {
        // Regression: AttachBody must NOT carry any observability fields.
        // The poll path is the single ingestion site for the
        // per-active-run snapshot; session-open stays a thin
        // identity/resume body.
        let body = sample_attach_body();
        let v = serde_json::to_value(&body).unwrap();
        let keys: std::collections::BTreeSet<_> = v.as_object().unwrap().keys().cloned().collect();
        let expected: std::collections::BTreeSet<_> = [
            "version",
            "os",
            "arch",
            "status",
            "in_flight_run",
            "project_slug",
            "host_label",
            "agent_versions",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            keys, expected,
            "AttachBody serialised key set drifted: {keys:?}"
        );
    }

    #[test]
    fn poll_status_from_wire_omits_observability_fields() {
        // Pre-observability shape: only status / in_flight_run / ts.
        let s = PollStatus::from_wire(WireStatus::Idle, None);
        let v = serde_json::to_value(&s).unwrap();
        let obj = v.as_object().unwrap();
        let keys: std::collections::BTreeSet<_> = obj.keys().cloned().collect();
        let expected: std::collections::BTreeSet<_> = ["status", "in_flight_run", "ts"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            keys, expected,
            "PollStatus::from_wire wire shape drifted: {keys:?}"
        );
    }

    #[test]
    fn poll_status_feature_off_matches_legacy_shape() {
        let snap = crate::daemon::state::ObservabilitySnapshot::default();
        let s = PollStatus::from_state(WireStatus::Busy, Some(Uuid::nil()), false, snap, 0);
        let v = serde_json::to_value(&s).unwrap();
        let obj = v.as_object().unwrap();
        // No new fields when feature is off — this is the strict back-compat
        // guarantee.
        assert!(!obj.contains_key("observed_run_id"));
        assert!(!obj.contains_key("last_event_at"));
        assert!(!obj.contains_key("agent_pid"));
        assert!(!obj.contains_key("approvals_pending"));
        assert!(!obj.contains_key("tokens"));
    }

    #[test]
    fn poll_status_feature_on_idle_serialises_observed_run_id_null() {
        let snap = crate::daemon::state::ObservabilitySnapshot::default();
        let s = PollStatus::from_state(WireStatus::Idle, None, true, snap, 0);
        let v = serde_json::to_value(&s).unwrap();
        let obj = v.as_object().unwrap();
        assert!(obj.contains_key("observed_run_id"));
        assert_eq!(obj.get("observed_run_id"), Some(&serde_json::Value::Null));
        // approvals_pending is always populated when feature is on.
        assert_eq!(obj.get("approvals_pending"), Some(&serde_json::json!(0)));
        // No event has fired yet — descriptive scalars stay absent.
        assert!(!obj.contains_key("last_event_at"));
        assert!(!obj.contains_key("agent_pid"));
    }

    #[test]
    fn poll_status_feature_on_busy_serialises_populated_fields() {
        use crate::daemon::observability::TokenUsage as InnerTokenUsage;
        let rid = Uuid::new_v4();
        let snap = crate::daemon::state::ObservabilitySnapshot {
            last_event_at: Some(Utc::now()),
            last_event_kind: Some("codex/event/token_count".into()),
            last_event_summary: Some("running".into()),
            agent_pid: Some(4242),
            agent_subprocess_alive: Some(true),
            tokens: Some(InnerTokenUsage {
                input: 100,
                output: 200,
                total: 300,
            }),
            model: Some("gpt-5.1-codex".into()),
            turn_count: Some(2),
            last_exec_command: None,
        };
        let s = PollStatus::from_state(WireStatus::Busy, Some(rid), true, snap, 1);
        let v = serde_json::to_value(&s).unwrap();
        let obj = v.as_object().unwrap();
        assert_eq!(obj.get("observed_run_id"), Some(&serde_json::json!(rid)));
        assert_eq!(obj.get("agent_pid"), Some(&serde_json::json!(4242)));
        assert_eq!(
            obj.get("agent_subprocess_alive"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(obj.get("approvals_pending"), Some(&serde_json::json!(1)));
        assert_eq!(obj.get("turn_count"), Some(&serde_json::json!(2)));
        assert_eq!(obj.get("model"), Some(&serde_json::json!("gpt-5.1-codex")));
        let tokens = obj.get("tokens").unwrap().as_object().unwrap();
        assert_eq!(tokens.get("input"), Some(&serde_json::json!(100)));
        assert_eq!(tokens.get("output"), Some(&serde_json::json!(200)));
        assert_eq!(tokens.get("total"), Some(&serde_json::json!(300)));
    }
}
