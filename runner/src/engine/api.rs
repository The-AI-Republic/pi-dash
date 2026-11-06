//! The server contract of the built-in engine (design §4.3, §5, §8), in one
//! place so the client and the server PR stay aligned.
//!
//! | Call | Auth | Request | Response |
//! | ---- | ---- | ------- | -------- |
//! | `POST /api/v1/auth/device/start/` | none | `{"scope": "engine"}` (engine-only grant) | as for a normal grant |
//! | `POST /api/v1/auth/device/token/` (engine grant) | `Authorization: Bearer <mt_>` | `{"device_code"}` | `{"engine_credential": {"token": "ec_…", "expires_at"}, "engine_sign_in_state"}` |
//! | `POST /api/v1/auth/device/token/`, `POST /api/v1/auth/machine-token/` (normal sign-in) | as today | as today | may also carry `engine_credential` (and `engine_sign_in_state`) |
//! | `GET /api/v1/runner/engine/profile/` | `Authorization: Bearer <mt_>` | — | [`EngineProfile`] |
//! | `POST /api/v1/runner/engine/model-token/` | `Authorization: Bearer <ec_>` | `{}` | [`ModelTokenResponse`]; errors `{"reason_code", "detail"?}` |
//! | `POST /api/v1/runner/engine/credential/revoke/` | `Authorization: Bearer <mt_>` | `{}` | 2xx (401 = already gone) |
//! | `GET /api/v1/runner/dev-machines/<id>/` | `Authorization: Bearer <mt_>` | — | [`MachinePayload`]: the pause state and this machine's runner rows in the token's workspace (revoked rows included) |
//! | `PATCH /api/v1/runner/dev-machines/<id>/` | `Authorization: Bearer <mt_>` | `{"engine_paused_by_owner": bool}` | 2xx, ideally with the [`MachinePayload`] |
//! | `POST /api/v1/runner/runners/` (engine runner) | `X-Api-Key: <mt_>` | the usual body plus `"agent_kind": "engine"`; `name` is not sent | the usual enrol response |
//!
//! The machine session's poll body (`POST …/dev-machines/<id>/sessions/<sid>/poll`)
//! carries the machine-level `engine_state` (see
//! `crate::cloud::protocol::MachinePollBody`). The session's `welcome` may
//! carry the pause fields of [`MachinePayload`]; the daemon caches them.

use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

pub use pidash_ipc::protocol::EngineSignInState;

pub const DEVICE_START_PATH: &str = "/api/v1/auth/device/start/";
pub const DEVICE_TOKEN_PATH: &str = "/api/v1/auth/device/token/";
/// `scope` of an engine-only device grant.
pub const ENGINE_GRANT_SCOPE: &str = "engine";
pub const PROFILE_PATH: &str = "/api/v1/runner/engine/profile/";
pub const MODEL_TOKEN_PATH: &str = "/api/v1/runner/engine/model-token/";
pub const CREDENTIAL_REVOKE_PATH: &str = "/api/v1/runner/engine/credential/revoke/";
/// `agent_kind` sent when creating an engine runner.
pub const ENGINE_AGENT_KIND: &str = "engine";

pub fn dev_machine_path(dev_machine_id: Uuid) -> String {
    format!("/api/v1/runner/dev-machines/{dev_machine_id}/")
}

/// Why the engine cannot run. The server's codes, plus two local ones.
pub mod reason {
    pub const ENGINE_SIGN_IN_REQUIRED: &str = "engine_sign_in_required";
    pub const GATEWAY_SESSION_REVOKED: &str = "gateway_session_revoked";
    pub const WALLET_EMPTY: &str = "wallet_empty";
    pub const PROFILE_UNAVAILABLE: &str = "profile_unavailable";
    pub const ENGINE_DISABLED: &str = "engine_disabled";
    /// Local: no engine binary was found at daemon start.
    pub const ENGINE_NOT_INSTALLED: &str = "engine_not_installed";
    /// Local: the profile has not been fetched yet (offline, or no
    /// workspace signed in).
    pub const PROFILE_PENDING: &str = "profile_pending";
}

/// The engine credential as the server issues it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssuedEngineCredential {
    pub token: String,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

/// Describe a success body that did not parse without quoting it: these
/// bodies carry the engine credential or a model token, and serde's message
/// can quote a value too.
pub(crate) fn unparsable(e: &serde_json::Error) -> String {
    format!(
        "unexpected response from the server ({:?} error at line {} column {})",
        e.classify(),
        e.line(),
        e.column()
    )
}

/// Read an `engine_sign_in_state` the server sent, mapping a value this
/// runner does not know (a newer server) to `None` instead of failing the
/// whole response.
pub(crate) fn lenient_sign_in_state<'de, D>(d: D) -> Result<Option<EngineSignInState>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(v.and_then(|v| serde_json::from_value(v).ok()))
}

/// Body of a redeemed engine-only grant.
#[derive(Debug, Clone, Deserialize)]
pub struct EngineGrantResponse {
    pub engine_credential: IssuedEngineCredential,
    #[serde(default, deserialize_with = "lenient_sign_in_state")]
    pub engine_sign_in_state: Option<EngineSignInState>,
}

/// One poll of an engine-only grant (RFC 8628).
#[derive(Debug)]
pub enum GrantPoll {
    Granted(EngineGrantResponse),
    /// `authorization_pending`: not approved yet.
    Pending,
    /// `slow_down`: not approved yet, and poll 5 s less often.
    SlowDown,
}

/// `GET /api/v1/runner/engine/profile/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineProfile {
    pub available: bool,
    #[serde(default)]
    pub lane: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub reason_code: String,
    #[serde(default, deserialize_with = "lenient_sign_in_state")]
    pub engine_sign_in_state: Option<EngineSignInState>,
}

/// `POST /api/v1/runner/engine/model-token/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelTokenResponse {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

/// The dev machine as the server returns it to its daemon:
/// `GET`/`PATCH /api/v1/runner/dev-machines/<id>/` and, for the pause
/// fields, the machine session's `welcome`. Every field is optional so a
/// partial body (an older server, a welcome frame) still parses.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachinePayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_paused_by_owner: Option<bool>,
    /// Empty or absent: not paused automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_auto_pause_reason: Option<String>,
    /// This machine's runner rows in the token's workspace, revoked rows
    /// included, so the daemon can reconcile its blocks (design §5).
    /// `None` when the body does not list them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runners: Option<Vec<MachineRunnerRow>>,
}

impl MachinePayload {
    /// The pause state, when the body carries it.
    pub fn pause(&self) -> Option<EnginePause> {
        let paused_by_owner = self.engine_paused_by_owner?;
        Some(EnginePause {
            paused_by_owner,
            auto_pause_reason: self
                .engine_auto_pause_reason
                .clone()
                .filter(|r| !r.trim().is_empty()),
        })
    }
}

/// The machine's engine pause state (design §8): the owner's switch and the
/// server's automatic reason, kept apart.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnginePause {
    pub paused_by_owner: bool,
    pub auto_pause_reason: Option<String>,
}

/// One runner row in [`MachinePayload::runners`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineRunnerRow {
    pub id: Uuid,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub workspace_slug: String,
    #[serde(default)]
    pub project_identifier: String,
    /// `"engine"` for a built-in engine runner.
    #[serde(default)]
    pub agent_kind: String,
    /// `desktop_bundled` marks an engine runner too (the stored value kept
    /// for compatibility, design §5).
    #[serde(default)]
    pub provisioning: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub status: String,
}

impl MachineRunnerRow {
    pub fn is_engine(&self) -> bool {
        self.agent_kind == ENGINE_AGENT_KIND || self.provisioning == "desktop_bundled"
    }

    pub fn is_revoked(&self) -> bool {
        self.revoked_at.is_some() || self.status == "revoked"
    }
}

/// An engine call the server refused.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum EngineApiError {
    /// The server answered with a reason code.
    #[error("{reason_code} (HTTP {status}){}", detail.as_deref().map(|d| format!(": {d}")).unwrap_or_default())]
    Refused {
        status: u16,
        reason_code: String,
        detail: Option<String>,
    },
    /// Anything else: transport errors, unexpected bodies.
    #[error("{0}")]
    Other(String),
}

impl EngineApiError {
    pub(crate) fn is_workspace_auth_failure(&self) -> bool {
        matches!(self, Self::Refused { status: 401 | 403, reason_code, .. } if workspace_auth_code(reason_code))
    }

    pub fn reason_code(&self) -> Option<&str> {
        match self {
            Self::Refused { reason_code, .. } => Some(reason_code),
            Self::Other(_) => None,
        }
    }
}

fn workspace_auth_code(code: &str) -> bool {
    matches!(
        code,
        "machine_token_expired"
            | "machine_token_revoked"
            | "machine_token_invalid"
            | "membership_revoked"
    )
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(default)]
    reason_code: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    detail: Option<String>,
}

/// Map a non-2xx answer to an [`EngineApiError`]. A 401 without a code is
/// an engine credential the server no longer accepts.
fn refused(status: StatusCode, body: &str) -> EngineApiError {
    let parsed: Option<ErrorBody> = serde_json::from_str(body).ok();
    let (code, detail) = match parsed {
        Some(b) => (
            b.reason_code
                .or(b.error)
                .or_else(|| b.detail.clone().filter(|code| workspace_auth_code(code))),
            b.detail,
        ),
        None => (
            None,
            (!body.trim().is_empty()).then(|| body.trim().to_string()),
        ),
    };
    let reason_code = code.unwrap_or_else(|| {
        if status == StatusCode::UNAUTHORIZED {
            reason::ENGINE_SIGN_IN_REQUIRED.to_string()
        } else {
            format!("http_{}", status.as_u16())
        }
    });
    EngineApiError::Refused {
        status: status.as_u16(),
        reason_code,
        detail,
    }
}

/// HTTP client for the engine endpoints.
#[derive(Clone)]
pub struct EngineApi {
    http: reqwest::Client,
    cloud_url: String,
}

impl EngineApi {
    pub fn new(cloud_url: &str) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            cloud_url: cloud_url.trim_end_matches('/').to_string(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.cloud_url)
    }

    async fn json<T: for<'de> Deserialize<'de>>(
        resp: reqwest::Response,
    ) -> Result<T, EngineApiError> {
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| EngineApiError::Other(e.to_string()))?;
        if !status.is_success() {
            return Err(refused(status, &body));
        }
        serde_json::from_str(&body).map_err(|e| EngineApiError::Other(unparsable(&e)))
    }

    async fn empty(resp: reqwest::Response) -> Result<(), EngineApiError> {
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let body = resp.text().await.unwrap_or_default();
        Err(refused(status, &body))
    }

    /// The model profile, with the daemon (machine) credential.
    pub async fn profile(&self, machine_token: &str) -> Result<EngineProfile, EngineApiError> {
        let resp = self
            .http
            .get(self.url(PROFILE_PATH))
            .bearer_auth(machine_token)
            .send()
            .await
            .map_err(|e| EngineApiError::Other(e.to_string()))?;
        Self::json(resp).await
    }

    /// A fresh model token, with the engine credential.
    pub async fn model_token(
        &self,
        engine_credential: &str,
    ) -> Result<ModelTokenResponse, EngineApiError> {
        let resp = self
            .http
            .post(self.url(MODEL_TOKEN_PATH))
            .bearer_auth(engine_credential)
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|e| EngineApiError::Other(e.to_string()))?;
        Self::json(resp).await
    }

    /// Revoke this machine's engine credential (`logout --engine`). A 401
    /// means there was nothing left to revoke.
    pub async fn revoke_credential(&self, machine_token: &str) -> Result<(), EngineApiError> {
        let resp = self
            .http
            .post(self.url(CREDENTIAL_REVOKE_PATH))
            .bearer_auth(machine_token)
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|e| EngineApiError::Other(e.to_string()))?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            return Ok(());
        }
        Self::empty(resp).await
    }

    /// `pidash engine pause` / `resume`. Returns the machine as the server
    /// answered it; a 2xx with no parseable body yields a payload holding
    /// only the switch that was set.
    pub async fn set_paused(
        &self,
        machine_token: &str,
        dev_machine_id: Uuid,
        paused: bool,
    ) -> Result<MachinePayload, EngineApiError> {
        let resp = self
            .http
            .patch(self.url(&dev_machine_path(dev_machine_id)))
            .bearer_auth(machine_token)
            .json(&serde_json::json!({ "engine_paused_by_owner": paused }))
            .send()
            .await
            .map_err(|e| EngineApiError::Other(e.to_string()))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(refused(status, &body));
        }
        let mut payload: MachinePayload = serde_json::from_str(&body).unwrap_or_default();
        if payload.engine_paused_by_owner.is_none() {
            payload.engine_paused_by_owner = Some(paused);
        }
        Ok(payload)
    }

    /// The machine with its pause state and runner rows (reconcile).
    pub async fn machine(
        &self,
        machine_token: &str,
        dev_machine_id: Uuid,
    ) -> Result<MachinePayload, EngineApiError> {
        let resp = self
            .http
            .get(self.url(&dev_machine_path(dev_machine_id)))
            .bearer_auth(machine_token)
            .send()
            .await
            .map_err(|e| EngineApiError::Other(e.to_string()))?;
        Self::json(resp).await
    }

    /// Start an engine-only device grant.
    pub async fn start_engine_grant(&self) -> Result<serde_json::Value, EngineApiError> {
        let resp = self
            .http
            .post(self.url(DEVICE_START_PATH))
            .json(&serde_json::json!({ "scope": ENGINE_GRANT_SCOPE }))
            .send()
            .await
            .map_err(|e| EngineApiError::Other(e.to_string()))?;
        Self::json(resp).await
    }

    /// One redeem attempt of an engine-only grant, with the machine token
    /// that binds the credential to this machine. `Ok(None)` while the user
    /// has not approved yet (`authorization_pending`, `slow_down`).
    pub async fn redeem_engine_grant(
        &self,
        device_code: &str,
        machine_token: &str,
    ) -> Result<GrantPoll, EngineApiError> {
        let resp = self
            .http
            .post(self.url(DEVICE_TOKEN_PATH))
            .bearer_auth(machine_token)
            .json(&serde_json::json!({ "device_code": device_code }))
            .send()
            .await
            .map_err(|e| EngineApiError::Other(e.to_string()))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| EngineApiError::Other(e.to_string()))?;
        if status.is_success() {
            return serde_json::from_str(&body)
                .map(GrantPoll::Granted)
                .map_err(|e| EngineApiError::Other(unparsable(&e)));
        }
        let err = refused(status, &body);
        match err.reason_code() {
            Some("authorization_pending") => Ok(GrantPoll::Pending),
            Some("slow_down") => Ok(GrantPoll::SlowDown),
            _ => Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unparsable_bodies_are_not_quoted_and_unknown_states_are_tolerated() {
        let e = serde_json::from_str::<EngineGrantResponse>(
            r#"{"engine_credential": {"token": 42, "secret": "ec_do_not_print"}}"#,
        )
        .unwrap_err();
        let msg = unparsable(&e);
        assert!(!msg.contains("ec_do_not_print"), "{msg}");
        let e = serde_json::from_str::<EngineGrantResponse>(r#"{"engine_credential": "ec_x"}"#)
            .unwrap_err();
        assert!(!unparsable(&e).contains("ec_x"));

        let p: EngineProfile =
            serde_json::from_str(r#"{"available": true, "engine_sign_in_state": "something_new"}"#)
                .unwrap();
        assert_eq!(p.engine_sign_in_state, None);
        let p: EngineProfile =
            serde_json::from_str(r#"{"available": true, "engine_sign_in_state": null}"#).unwrap();
        assert_eq!(p.engine_sign_in_state, None);
    }

    #[test]
    fn refusals_carry_the_server_reason_code() {
        let e = refused(
            StatusCode::PAYMENT_REQUIRED,
            r#"{"reason_code":"wallet_empty","detail":"top up"}"#,
        );
        assert_eq!(e.reason_code(), Some(reason::WALLET_EMPTY));
        assert!(e.to_string().contains("top up"));
        // A bare 401 is a credential the server no longer accepts.
        let e = refused(StatusCode::UNAUTHORIZED, "");
        assert_eq!(e.reason_code(), Some(reason::ENGINE_SIGN_IN_REQUIRED));
        // RFC 8628 errors use `error`.
        let e = refused(
            StatusCode::BAD_REQUEST,
            r#"{"error":"authorization_pending"}"#,
        );
        assert_eq!(e.reason_code(), Some("authorization_pending"));
    }

    #[test]
    fn machine_payload_parses_pause_and_runner_rows() {
        let p: MachinePayload = serde_json::from_str(
            r#"{"id":"00000000-0000-0000-0000-000000000009","engine_paused_by_owner":false,
                "engine_auto_pause_reason":"wallet_empty",
                "runners":[{"id":"00000000-0000-0000-0000-000000000001","name":"builtin-h-acme-web",
                  "workspace_slug":"acme","project_identifier":"WEB","provisioning":"desktop_bundled",
                  "revoked_at":"2026-11-04T10:00:00Z"},
                  {"id":"00000000-0000-0000-0000-000000000002","agent_kind":"codex","provisioning":"manual"}]}"#,
        )
        .unwrap();
        assert_eq!(
            p.pause(),
            Some(EnginePause {
                paused_by_owner: false,
                auto_pause_reason: Some(reason::WALLET_EMPTY.into())
            })
        );
        let rows = p.runners.unwrap();
        assert!(rows[0].is_engine() && rows[0].is_revoked());
        assert!(!rows[1].is_engine() && !rows[1].is_revoked());
        // A welcome frame without the fields carries no pause state.
        let w: MachinePayload =
            serde_json::from_str(r#"{"type":"welcome","dev_machine_id":null}"#).unwrap();
        assert_eq!(w.pause(), None);
        let empty_reason: MachinePayload = serde_json::from_str(
            r#"{"engine_paused_by_owner":true,"engine_auto_pause_reason":""}"#,
        )
        .unwrap();
        assert_eq!(empty_reason.pause().unwrap().auto_pause_reason, None);
    }

    #[test]
    fn profile_parses_the_documented_shape() {
        let p: EngineProfile = serde_json::from_str(
            r#"{"available":true,"lane":"openhub","base_url":"https://gw/v1","model":"gpt-5.5",
                "reason_code":"","engine_sign_in_state":"active"}"#,
        )
        .unwrap();
        assert!(p.available);
        assert_eq!(p.engine_sign_in_state, Some(EngineSignInState::Active));
        let p: EngineProfile =
            serde_json::from_str(r#"{"available":false,"reason_code":"engine_disabled"}"#).unwrap();
        assert_eq!(p.reason_code, reason::ENGINE_DISABLED);
    }
}
