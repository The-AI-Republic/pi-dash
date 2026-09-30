//! Assistant permission + throttle surface (D-06, stage 5).
//!
//! Ports the permission closure of `apps/api/pi_dash/assistant/`:
//!
//! * [`perm`] — `views/_base.py:1-34` (member gate, owned-thread scope)
//!   plus the SSE resolve matrix in `views/events.py:28-41`.
//! * [`throttles`] — the six `UserRateThrottle` subclasses in
//!   `views/messages.py:29-42`, `views/llm_config.py:81,111`,
//!   `views/stt_config.py:84`, `views/transcribe.py:57`,
//!   `views/agent_profile.py:67` with rates from
//!   `pi_dash/settings/common.py:93-117`.
//!
//! Fixture id F-A6-07 (`rust-api/fixtures/assistant/perms.json`).
//! Shape of the port: pure decision logic over already-fetched rows. Role
//! fetching (`WorkspaceMember` lookup) and thread fetching stay with the
//! handler layer, which owns the only scoped database handle (tenancy rule);
//! this module decides and renders, exactly like the Python view helpers.
//!
//! [`llm_config`] owns the BYOK LLM-config + title-generation HTTP shell
//! (PIDASHCONV-256) and [`stt_config`] the BYO speech-to-text config shell
//! (PIDASHCONV-256); [`common`] holds their shared request edge and
//! [`kms`] the production KMS wire.
//!
//! Handlers C (PIDASHCONV-257) adds the HTTP shell the handlers share
//! plus three endpoint modules:
//!
//! * [`transcribe`] — `views/transcribe.py:1-194` (multipart forward).
//! * [`agent_profile`] — `views/agent_profile.py:1-126` (desktop-only
//!   profile + token).
//! * [`mcp_servers`] — `views/mcp_servers.py:1-165` (per-user CRUD).
//! * [`multipart`] — the transcribe framing codec (no `multipart`
//!   feature on the pinned `axum`/`reqwest`; foundation `Cargo.toml`
//!   files are read-only for port issues).
//! * [`governor`] — the in-process DRF throttle store the handler layer
//!   owns (no shared cache handle exists in `AppState`).
//!
//! [`routes`] merges the owned paths; registration stays the cutover
//! granularity (unowned methods proxy to Django). Sibling handler
//! issues merge their own routers into [`routes`]; merges keep both
//! sides.

pub mod agent_profile;
pub mod common;
pub mod governor;
pub mod kms;
pub mod llm_config;
pub mod mcp_servers;
pub mod multipart;
pub mod perm;
pub mod stt_config;
pub mod throttles;
pub mod transcribe;

use axum::Router;

use crate::state::AppState;

/// Owned D-06 assistant routes (cutover granularity: registered paths
/// serve from Rust, everything else keeps proxying). Sibling handler
/// issues extend this merge; merges keep both sides.
pub fn routes() -> Router<AppState> {
    llm_config::routes()
        .merge(stt_config::routes())
        .merge(transcribe::routes())
        .merge(agent_profile::routes())
        .merge(mcp_servers::routes())
}

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::middleware::SessionHandle;

/// Handler failure with its exact status + body (the `loop::user`
/// shape: 401 from the shared license body, 429 from the throttle
/// rewrite, 500 from the shared server-error body).
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated` (anonymous on a guarded endpoint).
    Unauthorized,
    /// 429, the `auth_exception_handler` rewrite of DRF's `Throttled`.
    Throttled,
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    /// The throttle denial (handlers call this after a failed
    /// [`governor`] check).
    pub fn throttled() -> Self {
        Denial::Throttled
    }

    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                crate::license::UNAUTHENTICATED_BODY.to_owned(),
            ),
            Denial::Throttled => (
                StatusCode::TOO_MANY_REQUESTS,
                throttles::RATE_LIMIT_BODY.to_owned(),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                crate::license::SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("static denial response")
    }
}

/// The primary pool, or the 500 when the state carries none (same shape
/// as the `loop` handlers).
pub fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// `request.user` or the 401. Hash-verified DRF session semantics via
/// the shared license plumbing (read-only): bad session,
/// unknown/inactive user, or hash mismatch is anonymous
/// (`BaseAPIView.authentication_classes =
/// [BaseSessionAuthentication]`, `permission_classes =
/// [IsAuthenticated]`).
pub async fn request_actor(
    state: &AppState,
    pool: &sqlx::PgPool,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<crate::license::Actor, Denial> {
    // `resolve_actor` only ever errs with the license-layer 500, so the
    // mapping is exact (the `loop::user` precedent).
    match crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
    {
        Ok(Some(actor)) => Ok(actor),
        Ok(None) => Err(Denial::Unauthorized),
        Err(_) => Err(Denial::ServerError),
    }
}

/// Render an exact JSON body with its status. Insertion order is
/// preserved by the crate's `serde_json/preserve_order`, so DRF field
/// order survives rendering.
pub fn json_body(status: StatusCode, body: &serde_json::Value) -> Response {
    let rendered = serde_json::to_string(body).expect("assistant body serializes");
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(rendered))
        .expect("assistant response")
}

/// Alias for error envelopes (same rendering, distinct call sites).
pub fn error_response(status: StatusCode, body: &serde_json::Value) -> Response {
    json_body(status, body)
}

/// Render a caught `AssistantError` as its code/detail/status
/// (`except AssistantError as exc → (exc.code, exc.detail,
/// exc.http_status)`).
pub fn assistant_error_response(
    error: &pidash_types::assistant::errors::AssistantError,
) -> Response {
    let status = StatusCode::from_u16(error.http_status()).unwrap_or(StatusCode::OK);
    json_body(
        status,
        &serde_json::json!({"error": error.code(), "detail": error.detail()}),
    )
}

/// Crypto failure split: data problems (`NotConfigured`, i.e. Python
/// raising `AssistantNotConfigured`) versus operational problems
/// (`Transport`, i.e. Python letting the error propagate to the generic
/// 500).
#[derive(Debug)]
pub enum SecretError {
    /// Renders as the wrapped `AssistantError`.
    Assistant(pidash_types::assistant::errors::AssistantError),
    /// Answers the generic 500 (never an `AssistantError` in Python).
    Transport(String),
}

/// `crypto.encrypt` under the process env config
/// (`crypto.py:224-225`). The KMS wire refuses: the contract backends
/// are fernet (or unconfigured), and the workspace carries no AWS
/// transport for port issues — a configured-KMS call fails closed to
/// the 500 instead of hanging on a network dial.
pub fn encrypt_secret(plaintext: &str) -> Result<Vec<u8>, SecretError> {
    use pidash_services::assistant::crypto;
    crypto::encrypt(&crypto::CryptoConfig::from_env(), plaintext, &RefusingKms).map_err(|err| {
        match err {
            crypto::CryptoError::NotConfigured(error) => SecretError::Assistant(error),
            crypto::CryptoError::Transport(message) => SecretError::Transport(message.to_string()),
        }
    })
}

/// `crypto.decrypt` under the process env config
/// (`crypto.py:228-229`); same refusing wire as [`encrypt_secret`].
pub fn decrypt_secret(token: &[u8]) -> Result<String, SecretError> {
    use pidash_services::assistant::crypto;
    crypto::decrypt(&crypto::CryptoConfig::from_env(), token, &RefusingKms).map_err(|err| match err
    {
        crypto::CryptoError::NotConfigured(error) => SecretError::Assistant(error),
        crypto::CryptoError::Transport(message) => SecretError::Transport(message.to_string()),
    })
}

/// KMS wire that refuses (`KmsTransport`, `crypto.py:86-96`): only
/// constructed, never called, unless an operator configures the KMS
/// backend — in which case the call fails closed to the generic 500
/// rather than dialing AWS from a handler.
struct RefusingKms;

impl pidash_services::assistant::crypto::KmsTransport for RefusingKms {
    fn encrypt(
        &self,
        _key_id: &str,
        _plaintext: &[u8],
    ) -> Result<Vec<u8>, pidash_services::assistant::crypto::KmsTransportError> {
        Err(
            pidash_services::assistant::crypto::KmsTransportError::transport(
                "no KMS transport in the Rust handlers",
            ),
        )
    }

    fn decrypt(
        &self,
        _key_id: &str,
        _ciphertext: &[u8],
    ) -> Result<Vec<u8>, pidash_services::assistant::crypto::KmsTransportError> {
        Err(
            pidash_services::assistant::crypto::KmsTransportError::transport(
                "no KMS transport in the Rust handlers",
            ),
        )
    }

    fn re_encrypt(
        &self,
        _ciphertext: &[u8],
        _destination_key_id: &str,
    ) -> Result<Vec<u8>, pidash_services::assistant::crypto::KmsTransportError> {
        Err(
            pidash_services::assistant::crypto::KmsTransportError::transport(
                "no KMS transport in the Rust handlers",
            ),
        )
    }
}
