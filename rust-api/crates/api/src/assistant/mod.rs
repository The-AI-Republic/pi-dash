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
//! Handlers A (PIDASHCONV-255):
//!
//! * [`threads`] — thread list/create/detail (`views/threads.py:1-111`).
//! * [`messages`] — message list/create + cancel (`views/messages.py:1-128`).
//! * [`events`] — the SSE event stream (`views/events.py:1-116`).
//! * [`redis`] — cancel-signal / throttle-cache / live-tail touch points
//!   (transport via the merged Redis foundation; see that module).
//!
//! [`routes`] merges the owned paths; registration stays the cutover
//! granularity (unowned methods proxy to Django). Sibling handler
//! issues merge their own routers into [`routes`]; merges keep both
//! sides.

pub mod agent_profile;
pub mod common;
pub mod events;
pub mod governor;
pub mod kms;
pub mod llm_config;
pub mod mcp_servers;
pub mod messages;
pub mod model_seam;
pub mod multipart;
pub mod perm;
pub mod redis;
pub mod stt_config;
pub mod threads;
pub mod throttles;
pub mod transcribe;

use axum::Router;

use crate::state::AppState;
pub use pidash_compat::response::{json_value as error_response, json_value as json_body};

/// Owned D-06 assistant routes (cutover granularity: registered paths
/// serve from Rust, everything else keeps proxying). Sibling handler
/// issues extend this merge; merges keep both sides.
pub fn routes() -> Router<AppState> {
    threads::routes()
        .merge(messages::routes())
        .merge(events::routes())
        .merge(llm_config::routes())
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

/// Render a caught `AssistantError` as its code/detail/status
/// (`except AssistantError as exc → (exc.code, exc.detail,
/// exc.http_status)`).
pub fn assistant_error_response(
    error: &pidash_types::assistant::errors::AssistantError,
) -> Response {
    let status = StatusCode::from_u16(error.http_status()).unwrap_or(StatusCode::OK);
    json_body(
        status,
        serde_json::json!({"error": error.code(), "detail": error.detail()}),
    )
}
