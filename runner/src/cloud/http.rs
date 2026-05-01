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
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
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
    pub fn is_fatal_for_runner(&self) -> bool {
        matches!(
            self,
            TransportError::RefreshTokenReplayed
                | TransportError::MembershipRevoked
                | TransportError::RunnerRevoked
                | TransportError::RunnerIdMismatch
                | TransportError::InvalidRefreshToken
                | TransportError::SessionEvicted { .. }
        )
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
        let http = Client::builder()
            .pool_idle_timeout(Duration::from_secs(60))
            .timeout(Duration::from_secs(60))
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
    pub async fn rotate(
        &self,
        new_token: String,
        new_generation: u64,
    ) -> Result<()> {
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
    pub fn new(
        runner_id: Uuid,
        creds: CredentialsHandle,
        transport: SharedHttpTransport,
    ) -> Self {
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
        guard
            .access_token
            .clone()
            .ok_or(TransportError::Protocol("no access token after refresh".into()))
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
                    Err(err) => {
                        RefreshOutcome::Done(Err(TransportErrorCode::from(err)))
                    }
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
            server_time: parsed
                .welcome
                .server_time
                .unwrap_or_else(Utc::now),
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
        let resp = self
            .inner
            .transport
            .http()
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {token_raw}"))
            .json(&body)
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
                self.post_run_lifecycle(run_id, "accept", body, &idempotency_key).await
            }
            msg @ ClientMsg::RunStarted { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "started", body, &idempotency_key).await
            }
            msg @ ClientMsg::RunEvent { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_event(run_id, body, &idempotency_key).await
            }
            msg @ ClientMsg::ApprovalRequest { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "approvals", body, &idempotency_key).await
            }
            msg @ ClientMsg::RunAwaitingReauth { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "awaiting-reauth", body, &idempotency_key).await
            }
            msg @ ClientMsg::RunCompleted { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "complete", body, &idempotency_key).await
            }
            msg @ ClientMsg::RunPaused { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "pause", body, &idempotency_key).await
            }
            msg @ ClientMsg::RunFailed { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "fail", body, &idempotency_key).await
            }
            msg @ ClientMsg::RunCancelled { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "cancelled", body, &idempotency_key).await
            }
            msg @ ClientMsg::RunResumed { run_id, .. } => {
                let body = to_value(&msg)?;
                self.post_run_lifecycle(run_id, "resumed", body, &idempotency_key).await
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

    /// One-shot POST that auto-refreshes once on `401 access_token_expired`.
    async fn post_authed_with_retry<F: Fn(&Json) -> bool>(
        &self,
        url: &str,
        body: Json,
        idempotency_key: &str,
        _accept: F,
    ) -> Result<Json, TransportError> {
        let mut attempt: u8 = 0;
        loop {
            let token = self.ensure_access_token().await?;
            let req = self
                .inner
                .transport
                .http()
                .post(url)
                .header(AUTHORIZATION, format!("Bearer {}", token.raw))
                .header("Idempotency-Key", idempotency_key);
            let resp = req
                .json(&body)
                .send()
                .await
                .map_err(|e| TransportError::Network(e.to_string()))?;
            let status = resp.status();
            if status.is_success() {
                let v: Json = resp.json().await.unwrap_or(Json::Null);
                return Ok(v);
            }
            if status == StatusCode::UNAUTHORIZED && attempt == 0 {
                let txt = resp.text().await.unwrap_or_default();
                if txt.contains("access_token_expired") {
                    self.refresh().await?;
                    attempt += 1;
                    continue;
                }
                return Err(map_auth_error(status, &txt));
            }
            let txt = resp.text().await.unwrap_or_default();
            return Err(map_auth_error(status, &txt));
        }
    }
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
}

#[derive(Debug, Clone, Serialize)]
pub struct PollStatus {
    pub status: String,
    pub in_flight_run: Option<Uuid>,
    pub ts: DateTime<Utc>,
}

impl PollStatus {
    pub fn idle() -> Self {
        Self {
            status: "idle".to_string(),
            in_flight_run: None,
            ts: Utc::now(),
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
        }
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
    inline_acks: VecDeque<String>,
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
            inline_acks: VecDeque::new(),
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

    pub async fn run(mut self) -> Result<(), TransportError> {
        // 1. Bootstrap: ensure access token, open session.
        self.client.ensure_access_token().await?;
        let session = self.client.open_session(self.current_attach_body()).await?;
        // 2. Hand welcome (and optional resume_ack) to the mailbox so
        //    the existing per-runner handlers ingest them as today.
        let welcome_body = ServerMsg::Welcome {
            server_time: session.welcome.server_time.unwrap_or_else(Utc::now),
            heartbeat_interval_secs: session.welcome.long_poll_interval_secs.unwrap_or(25),
            protocol_version: session.welcome.protocol_version.unwrap_or(WIRE_VERSION),
        };
        let _ = self
            .mailbox
            .send(InboundEnvelope {
                stream_id: None,
                env: Envelope::for_runner(self.client.runner_id(), welcome_body),
            })
            .await;
        if let Some(resume_body) = session.resume_ack
            && let Ok(parsed) = serde_json::from_value::<ServerMsg>(resume_body.clone())
        {
            let _ = self
                .mailbox
                .send(InboundEnvelope {
                    stream_id: None,
                    env: Envelope::for_runner(self.client.runner_id(), parsed),
                })
                .await;
        }

        let mut backoff_secs = 1u64;
        loop {
            let shutdown = self.shutdown.clone();
            match tokio::select! {
                _ = shutdown.notified() => break,
                result = self.poll_once() => result,
            } {
                Ok(()) => {
                    backoff_secs = 1;
                }
                Err(err) if err.is_fatal_for_runner() => {
                    tracing::warn!(runner = %self.client.runner_id(), "fatal transport error: {err}");
                    return Err(err);
                }
                Err(err) if err.is_recoverable() => {
                    tracing::debug!(runner = %self.client.runner_id(), "recoverable transport error: {err}");
                    sleep(Duration::from_secs(backoff_secs)).await;
                    backoff_secs = (backoff_secs * 2).min(30);
                    let _ = self
                        .client
                        .open_session(self.current_attach_body())
                        .await;
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
        let status = PollStatus::from_wire(*self.status_rx.borrow(), *self.in_flight_rx.borrow());
        let resp = self.client.poll(acks, status).await?;
        for msg in resp.messages {
            self.dispatch(msg).await;
        }
        Ok(())
    }

    async fn dispatch(&mut self, msg: PollMessage) {
        let stream_id = msg.stream_id.clone();
        let runner_id = self.client.runner_id();
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

/// Public helper to enroll a runner — replaces the legacy `enroll.rs`
/// connection-flow body.
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

pub async fn write_runner_credentials(
    path: PathBuf,
    creds: RunnerCredentials,
) -> Result<(), TransportError> {
    let handle = CredentialsHandle::new(path, creds.clone());
    handle
        .rotate(
            creds.refresh_token.clone(),
            creds.refresh_token_generation,
        )
        .await
        .map_err(|e| TransportError::Network(e.to_string()))
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
        let err = map_auth_error(
            StatusCode::FORBIDDEN,
            "{\"error\":\"runner_id_mismatch\"}",
        );
        assert!(matches!(err, TransportError::RunnerIdMismatch));
    }

    #[test]
    fn fatal_classification() {
        assert!(TransportError::RunnerRevoked.is_fatal_for_runner());
        assert!(TransportError::RefreshTokenReplayed.is_fatal_for_runner());
        assert!(TransportError::SessionEvicted { reason: "x".into() }.is_fatal_for_runner());
        assert!(!TransportError::Network("net".into()).is_fatal_for_runner());
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
}
