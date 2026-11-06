//! Engine daemon endpoints (#2910, NEWPIDASH-50): profile, model-token
//! and credential-revoke, ported from
//! `apps/api/pi_dash/runner/views/engine.py`.
//!
//! All three are Django `APIView`s; the module preserves their DRF
//! shapes byte-for-byte, including the ones Python's serializer and
//! exception handler produce by accident:
//!
//! * `MachineTokenAuthentication` failures are the compact DRF
//!   `{"detail": code}` 401s (with `Bearer` + `Allow`) rendered by
//!   [`crate::runner_enroll::auth`]; a verified token that is not bound
//!   to a machine is the 403 `{"error", "detail"}` the view itself
//!   returns.
//! * The model-token endpoint has no DRF auth classes: the `ec_`
//!   credential is the auth, and its failures are the custom
//!   `{reason_code, error, detail}` shapes the view renders by hand.
//! * The model-token throttle is DRF's `initial()`-time check (before
//!   the view body): denial is the `Throttled` rewrite body
//!   (`error_code` 5900, 429).
//! * A dead credential pauses only when no live credential remains: a
//!   credential renewed behind the daemon's back keeps serving, and its
//!   engine must not be paused.
//!
//! Approximations are listed in the section at the end of this module.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use chrono::{DateTime, Utc};
use serde_json::Map;
use uuid::Uuid;

use pidash_db::entity::dev_machine;
use pidash_services::domain::engine_credential::{
    authenticate_engine_credential, engine_profile_for_user, is_legacy_desktop, AuthError,
};
use pidash_services::domain::engine_state::{apply_model_token_outcome, apply_profile_outcome};
use pidash_services::extensions::CredentialError;

use crate::assistant::governor;
use crate::assistant::throttles::{ThrottleSpec, RATE_LIMIT_BODY};
use crate::runner_enroll::auth::{authenticate_machine_token, ALLOW_GET, ALLOW_POST};
use crate::runner_runs::{bearer_token, json_response, pool_of, server_error};
use crate::state::AppState;

/// The engine's model-token throttle (`engine_model_token`,
/// `settings/common.py:117`, 30/minute). Defined here rather than in
/// `assistant/throttles.rs`: the assistant facet's `ALL_THROTTLES`
/// pins its own set and order, and this spec is the engine facet's.
pub const ENGINE_MODEL_TOKEN_THROTTLE: ThrottleSpec = ThrottleSpec {
    scope: "engine_model_token",
    requests: 30,
    window_secs: 60,
};

/// The profile's `{"error", "detail"}` body when the machine token is
/// not machine-bound.
const DEV_MACHINE_REQUIRED_BODY: &str = r#"{"error": "dev_machine_required", "detail": "This machine token is not bound to a computer."}"#;

/// The transient 503 body (same shape for an unclassified gateway
/// failure and an unknown mint failure).
const TRANSIENT_503_BODY: &str =
    r#"{"error": "gateway_unavailable", "detail": "Try again shortly."}"#;

/// The engine-disabled failure detail (`"The engine is turned off."`).
const ENGINE_OFF_DETAIL: &str = "The engine is turned off.";

/// The dead-credential detail: the daemon's fix is the engine sign-in.
const SIGN_IN_AGAIN_DETAIL: &str = "Sign the built-in engine in again (`pidash login --engine`).";

/// The wire code for a dead credential under every model-token failure
/// path that is not itself a pause reason.
pub const CODE_ENGINE_SIGN_IN_REQUIRED: &str = "engine_sign_in_required";

/// Peer address from the serve-installed `ConnectInfo` extension, when
/// present (`Option<ConnectInfo>` is not an extractor — only types
/// implementing `OptionalFromRequestParts` wrap in `Option`; the
/// `device_flow.rs` position).
pub struct PeerAddr(Option<std::net::SocketAddr>);

impl<S> axum::extract::FromRequestParts<S> for PeerAddr
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(PeerAddr(
            parts
                .extensions
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|info| info.0),
        ))
    }
}

/// Read a machine row for the profile's legacy skip and payload.
#[allow(clippy::result_large_err)]
async fn read_machine_row(
    sea: &impl sea_orm::ConnectionTrait,
    machine_id: Uuid,
) -> Result<Option<dev_machine::Model>, Response> {
    use sea_orm::EntityTrait as _;
    dev_machine::Entity::find_by_id(machine_id)
        .one(sea)
        .await
        .map_err(|_| server_error())
}

/// Whether the machine holds a live engine credential (unrevoked and
/// unexpired), re-checked at the moment of the pause.
async fn has_live_credential_for(
    sea: &impl sea_orm::ConnectionTrait,
    machine_id: Uuid,
    now: DateTime<Utc>,
) -> bool {
    use sea_orm::{ColumnTrait as _, EntityTrait as _, QueryFilter as _, QuerySelect as _};

    pidash_db::entity::engine_credential::Entity::find()
        .filter(pidash_db::entity::engine_credential::Column::DevMachineId.eq(machine_id))
        .filter(pidash_db::entity::engine_credential::Column::RevokedAt.is_null())
        .filter(pidash_db::entity::engine_credential::Column::ExpiresAt.gt(now.fixed_offset()))
        .limit(1)
        .one(sea)
        .await
        .map(|row| row.is_some())
        .unwrap_or(false)
}

/// The engine payload's fields as a map, in Django's key order.
fn payload_map(
    stored: &str,
    expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Map<String, serde_json::Value> {
    let payload = pidash_services::domain::engine_credential::engine_credential_payload(
        stored, expires_at, now,
    );
    payload.as_object().cloned().unwrap_or_default()
}

/// Merge the machine-bound engine fields after the auth's machine id:
/// `Ok(Some(machine_id))`, `Ok(None)` (no token / no machine-bound
/// token) or the auth denial response.
#[allow(clippy::result_large_err)]
async fn authed_machine_id(
    state: &AppState,
    headers: &HeaderMap,
    allow: &'static str,
) -> Result<Option<Uuid>, Response> {
    let pool = pool_of(state)?;
    let secret = state.settings().secret_key.clone();
    let auth = authenticate_machine_token(pool, secret.as_bytes(), headers, allow).await?;
    Ok(auth.and_then(|auth| auth.token.dev_machine_id))
}

/// Whether the user holds a BYOK LLM key (the seam's `has_key` input):
/// the same one-row read the profile endpoint makes for its own
/// `agent_model_profile` call.
#[allow(clippy::result_large_err)]
async fn llm_has_key(sea: &impl sea_orm::ConnectionTrait, user_id: Uuid) -> Result<bool, Response> {
    use sea_orm::{ColumnTrait as _, EntityTrait as _, QueryFilter as _, QuerySelect as _};

    #[derive(Debug, sea_orm::FromQueryResult)]
    struct LoadRow {
        api_key_encrypted: Option<Vec<u8>>,
    }
    let row = pidash_db::entity::assistant_user_llm_config::Entity::find()
        .filter(pidash_db::entity::assistant_user_llm_config::Column::UserId.eq(user_id))
        .select_only()
        .column(pidash_db::entity::assistant_user_llm_config::Column::ApiKeyEncrypted)
        .into_model::<LoadRow>()
        .one(sea)
        .await
        .map_err(|_| server_error())?;
    Ok(row
        .as_ref()
        .is_some_and(|row| pidash_db::assistant::models::has_secret(&row.api_key_encrypted)))
}

/// The model-token throttle identity: the credential's hash bucket when
/// a bearer is presented, else the client identity (DRF's `get_ident`:
/// the `X-Forwarded-For` value with all whitespace removed, else the
/// peer). The bucket key is internal; the two forms only need to be
/// stable per presenter.
pub fn engine_throttle_ident(
    headers: &HeaderMap,
    bearer: Option<&str>,
    peer: Option<&str>,
) -> String {
    if let Some(raw) = bearer {
        let mut hasher = sha2::Sha256::new();
        use sha2::Digest as _;
        hasher.update(raw.as_bytes());
        return format!("{:x}", hasher.finalize())[..32].to_owned();
    }
    if let Some(forwarded) = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
    {
        return forwarded.split_whitespace().collect::<Vec<_>>().join("");
    }
    peer.unwrap_or_default().to_owned()
}

/// The id the model-token spend audit logs (`_request_id`): the
/// `X-Request-Id` trimmed to 64 chars, else a fresh UUID hex. The
/// delta from Django's `[:64]` applied to whichever won is
/// unobservable: a UUID hex is 32 chars.
pub fn request_id_from(headers: &HeaderMap) -> String {
    match headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
    {
        Some(value) => value.chars().take(64).collect(),
        None => Uuid::new_v4().simple().to_string(),
    }
}

/// A setup failure of the model-token endpoint (everything except a
/// dead credential, which answers 401 with a different shape): a pause
/// reason plus its detail, or the pause-free transient.
pub enum ModelTokenFailure {
    /// Apply the reason, then answer `{"reason_code", "detail"}`.
    Reason { reason: String, detail: String },
    /// No pause: the daemon never saw a real gateway answer.
    Transient,
}

/// Classify a mint failure between the engine outcome and the transient
/// 503 (Django `_classify_gateway_error` over the exception name; the
/// seam arrives pre-classified).
pub fn classify_model_token_failure(error: &CredentialError) -> ModelTokenFailure {
    match error {
        CredentialError::Unavailable(unavailable) => ModelTokenFailure::Reason {
            reason: "profile_unavailable".to_owned(),
            detail: unavailable.reason.clone(),
        },
        CredentialError::WalletEmpty { detail } => ModelTokenFailure::Reason {
            reason: "wallet_empty".to_owned(),
            detail: detail.clone(),
        },
        CredentialError::Classified { code, .. } if code == "gateway_session_revoked" => {
            ModelTokenFailure::Reason {
                reason: "gateway_session_revoked".to_owned(),
                detail: code.clone(),
            }
        }
        CredentialError::Classified { .. } => ModelTokenFailure::Transient,
    }
}

/// The status Django answers a model-token failure reason with
/// (`engine.py:240-249`): `engine_sign_in_required` is unreachable
/// here (it answers 401 from the credential-error branch instead).
fn reason_status(reason: &str) -> StatusCode {
    match reason {
        "wallet_empty" => StatusCode::PAYMENT_REQUIRED,
        _ => StatusCode::CONFLICT,
    }
}

/// `POST` the engine's model token (`EngineModelTokenEndpoint`).
pub async fn engine_model_token(
    State(state): State<AppState>,
    PeerAddr(peer): PeerAddr,
    headers: HeaderMap,
) -> Response {
    // DRF throttles in `initial()`, before the view body (and with no
    // auth classes, before any credential work).
    let bearer = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| bearer_token(Some(value)));
    let ident = engine_throttle_ident(
        &headers,
        bearer,
        peer.map(|info| info.to_string()).as_deref(),
    );
    if !governor::throttle_check(ENGINE_MODEL_TOKEN_THROTTLE, &ident) {
        return json_response(StatusCode::TOO_MANY_REQUESTS, RATE_LIMIT_BODY);
    }
    let now = Utc::now();
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let secret = state.settings().secret_key.clone();
    let sea = pidash_db::pool::sea_orm_connection(pool);
    let raw = bearer.unwrap_or_default();
    let credential = match authenticate_engine_credential(&sea, secret.as_bytes(), raw, now).await {
        Ok(credential) => credential,
        Err(AuthError::Database(_)) => return server_error(),
        Err(AuthError::Credential(error)) => {
            let code = error.code.clone();
            if code == CODE_ENGINE_SIGN_IN_REQUIRED {
                if let Some(carried) = error.credential {
                    // `apply … engine_sign_in_required` only when no live
                    // credential remains for the machine: a credential
                    // renewed since the daemon's last report must not get
                    // its engine paused. Failure logs, never fails the 401.
                    if !has_live_credential_for(&sea, carried.dev_machine_id, now).await {
                        if let Err(inner) = apply_model_token_outcome(
                            &sea,
                            carried.dev_machine_id,
                            CODE_ENGINE_SIGN_IN_REQUIRED,
                            now,
                        )
                        .await
                        {
                            tracing::error!(
                                dev_machine_id = %carried.dev_machine_id,
                                error = %inner,
                                "model token pause update failed"
                            );
                        }
                    }
                }
            }
            tracing::warn!(code = %code, "engine.model_token_error");
            return json_response(
                StatusCode::UNAUTHORIZED,
                serde_json::to_string(&serde_json::json!({
                    "reason_code": CODE_ENGINE_SIGN_IN_REQUIRED,
                    "error": code,
                    "detail": SIGN_IN_AGAIN_DETAIL,
                }))
                .expect("static engine body"),
            );
        }
    };

    let managed_enabled = state.settings().managed_runner.enabled;
    let has_key = match llm_has_key(&sea, credential.user_id).await {
        Ok(has_key) => has_key,
        Err(response) => return response,
    };
    let profile = state
        .assistant_seam()
        .agent_model_profile(credential.user_id, has_key)
        .await;
    let (available, pause_reason) = engine_profile_for_user(managed_enabled, &profile);
    if !available {
        // The profile endpoint's to report: the seam's reason code for
        // a profile failure, else the instance switch's own words.
        let detail = if pause_reason == "profile_unavailable" {
            profile.reason_code.clone()
        } else {
            ENGINE_OFF_DETAIL.to_owned()
        };
        if apply_model_token_outcome(&sea, credential.dev_machine_id, &pause_reason, now)
            .await
            .is_err()
        {
            return server_error();
        }
        tracing::warn!(reason = %pause_reason, "engine.model_token_error");
        return model_token_reason_body(&pause_reason, &detail);
    }

    match state
        .assistant_seam()
        .agent_model_credential(credential.user_id)
        .await
    {
        Ok((token, expires_at)) => {
            if apply_model_token_outcome(&sea, credential.dev_machine_id, "", now)
                .await
                .is_err()
            {
                return server_error();
            }
            tracing::info!(
                request_id = %request_id_from(&headers),
                dev_machine_id = %credential.dev_machine_id,
                user_id = %credential.user_id,
                fingerprint = %credential.fingerprint,
                "engine.model_token_issued",
            );
            json_response(
                StatusCode::OK,
                serde_json::to_string(&serde_json::json!({
                    "token": token,
                    "expires_at": expires_at,
                }))
                .expect("static engine body"),
            )
        }
        Err(error) => {
            tracing::warn!(
                reason = %match &error {
                    CredentialError::Unavailable(unavailable) => unavailable.reason.clone(),
                    CredentialError::WalletEmpty { detail } => detail.clone(),
                    CredentialError::Classified { code, .. } => code.clone(),
                },
                "engine.model_token_error"
            );
            match classify_model_token_failure(&error) {
                ModelTokenFailure::Reason { reason, detail } => {
                    if apply_model_token_outcome(&sea, credential.dev_machine_id, &reason, now)
                        .await
                        .is_err()
                    {
                        return server_error();
                    }
                    model_token_reason_body(&reason, &detail)
                }
                ModelTokenFailure::Transient => {
                    json_response(StatusCode::SERVICE_UNAVAILABLE, TRANSIENT_503_BODY)
                }
            }
        }
    }
}

/// `{"reason_code", "detail"}` for a model-token failure reason.
fn model_token_reason_body(reason: &str, detail: &str) -> Response {
    json_response(
        reason_status(reason),
        serde_json::to_string(&serde_json::json!({
            "reason_code": reason,
            "detail": detail,
        }))
        .expect("static engine body"),
    )
}

/// `GET` the engine profile (`EngineProfileEndpoint`).
pub async fn engine_profile(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let now = Utc::now();
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let sea = pidash_db::pool::sea_orm_connection(pool);

    let Some(machine_id) = (match authed_machine_id(&state, &headers, ALLOW_GET).await {
        Ok(machine_id) => machine_id,
        Err(response) => return response,
    }) else {
        return json_response(StatusCode::FORBIDDEN, DEV_MACHINE_REQUIRED_BODY);
    };
    let Some(machine) = (match read_machine_row(&sea, machine_id).await {
        Ok(machine) => machine,
        Err(response) => return response,
    }) else {
        return json_response(StatusCode::FORBIDDEN, DEV_MACHINE_REQUIRED_BODY);
    };
    // The token's user is the machine's owner (auth guarantees it):
    // their profile is the machine's engine profile.
    let has_key = match llm_has_key(&sea, machine.owner_id).await {
        Ok(has_key) => has_key,
        Err(response) => return response,
    };
    let profile = state
        .assistant_seam()
        .agent_model_profile(machine.owner_id, has_key)
        .await;
    let (available, pause_reason) =
        engine_profile_for_user(state.settings().managed_runner.enabled, &profile);
    if !is_legacy_desktop(&machine.provisioning)
        && apply_profile_outcome(&sea, machine_id, &pause_reason, now)
            .await
            .is_err()
    {
        return server_error();
    }
    // The profile endpoint's answer for `reason_code`: its own
    // `engine_disabled` when the instance switch off is the pause,
    // empty while serving, else the profile's reason.
    let reason_code = if pause_reason == "engine_disabled" {
        "engine_disabled".to_owned()
    } else if available {
        String::new()
    } else if profile.reason_code.is_empty() {
        "profile_unavailable".to_owned()
    } else {
        profile.reason_code.clone()
    };
    let mut body = Map::new();
    body.insert("available".to_owned(), serde_json::Value::Bool(available));
    body.insert(
        "lane".to_owned(),
        serde_json::Value::String(profile.lane.clone()),
    );
    body.insert(
        "base_url".to_owned(),
        serde_json::Value::String(profile.base_url.clone()),
    );
    body.insert(
        "model".to_owned(),
        serde_json::Value::String(profile.model.clone()),
    );
    body.insert(
        "reason_code".to_owned(),
        serde_json::Value::String(reason_code),
    );
    body.insert(
        "pause_reason".to_owned(),
        serde_json::Value::String(pause_reason.to_string()),
    );
    body.insert(
        "top_up_url".to_owned(),
        serde_json::Value::String(profile.top_up_url.clone()),
    );
    for (key, value) in payload_map(
        &machine.engine_sign_in_state,
        machine
            .engine_credential_expires_at
            .map(|instant| instant.with_timezone(&Utc)),
        now,
    ) {
        body.insert(key, value);
    }
    json_response(
        StatusCode::OK,
        serde_json::to_string(&serde_json::Value::Object(body)).expect("static engine body"),
    )
}

/// `POST` the engine credential revoke
/// (`EngineCredentialRevokeEndpoint`): revoke and answer the payload.
pub async fn engine_credential_revoke(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let now = Utc::now();
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(response) => return response,
    };
    let sea = pidash_db::pool::sea_orm_connection(pool);

    let Some(machine_id) = (match authed_machine_id(&state, &headers, ALLOW_POST).await {
        Ok(machine_id) => machine_id,
        Err(response) => return response,
    }) else {
        return json_response(StatusCode::FORBIDDEN, DEV_MACHINE_REQUIRED_BODY);
    };
    // Idempotent: revoking a signed-out machine is a no-op that still
    // answers the payload.
    let revoked = match pidash_services::domain::engine_credential::revoke_engine_credential(
        &sea,
        machine_id,
        "signed_out",
        now,
    )
    .await
    {
        Ok(revoked) => revoked,
        Err(_) => return server_error(),
    };
    let Some(machine) = (match read_machine_row(&sea, machine_id).await {
        Ok(machine) => machine,
        Err(response) => return response,
    }) else {
        return json_response(StatusCode::FORBIDDEN, DEV_MACHINE_REQUIRED_BODY);
    };
    let mut body = Map::new();
    body.insert("ok".to_owned(), serde_json::Value::Bool(true));
    body.insert("revoked".to_owned(), serde_json::Value::Bool(revoked));
    for (key, value) in payload_map(
        &machine.engine_sign_in_state,
        machine
            .engine_credential_expires_at
            .map(|instant| instant.with_timezone(&Utc)),
        now,
    ) {
        body.insert(key, value);
    }
    json_response(
        StatusCode::OK,
        serde_json::to_string(&serde_json::Value::Object(body)).expect("static engine body"),
    )
}

// ---------------------------------------------------------------------------
// Approximations
// ---------------------------------------------------------------------------
//
// * A dead credential's `error` carries the credential error's code
//   unmodified: a revoked machine answers 401 with
//   `reason_code=engine_sign_in_required` and `error=dev_machine_revoked`,
//   then pauses only under the main code — exactly Django's call.
// * The profile's `reason_code` fallback (`or 'profile_unavailable'`)
//   triggers on an empty string like Python's `or`.
// * A classified gateway failure's 409 `detail` is the wire code, not
//   Django's exception class name: the seam has no exception class to
//   name, and `detail` from the opaque code is the deterministic share
//   of that value every provider agrees on.
// * The machine row is re-read after the machine-token auth, where
//   Django holds the auth-time joined row. The payload and legacy skip
//   see a moment-fresher row; the only concurrent writer of these
//   fields is this request's own credential issuance.
// * A DB failure mid-endpoint is the stock 500, like Django's HTML.
//   DB failures inside the dead-credential pause check log and still
//   answer the 401 (Django's `logger.exception` try/except).

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;

    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        pairs
            .iter()
            .map(|(key, value)| {
                (
                    axum::http::HeaderName::from_bytes(key.as_bytes()).expect("static header name"),
                    axum::http::HeaderValue::from_str(value).expect("static header"),
                )
            })
            .collect()
    }

    #[test]
    fn throttle_ident_buckets_per_presenter() {
        let no_headers = HeaderMap::new();
        // Bearer: the credential's hash bucket, not the raw value.
        let ident = engine_throttle_ident(&no_headers, Some("ec_abc123"), None);
        assert_eq!(ident.len(), 32);
        assert_ne!(ident, "ec_abc123");
        // A different credential lands in a different bucket; the same
        // one lands in the same bucket across calls.
        let again = engine_throttle_ident(&no_headers, Some("ec_abc123"), None);
        let other = engine_throttle_ident(&no_headers, Some("ec_other"), None);
        assert_eq!(ident, again);
        assert_ne!(ident, other);
        // No bearer: the client identity (XFF with all whitespace
        // removed, else the peer).
        let xff = headers(&[("x-forwarded-for", " 1.2.3.4 , 5.6.7.8 ")]);
        assert_eq!(
            engine_throttle_ident(&xff, None, Some("9.9.9.9")),
            "1.2.3.4,5.6.7.8"
        );
        assert_eq!(
            engine_throttle_ident(&no_headers, None, Some("9.9.9.9")),
            "9.9.9.9"
        );
        assert_eq!(engine_throttle_ident(&no_headers, None, None), "");
    }

    #[test]
    fn request_id_prefers_header_truncated_to_64() {
        let long = "x".repeat(100);
        let with_header = headers(&[("x-request-id", &long)]);
        assert_eq!(request_id_from(&with_header), "x".repeat(64));
        let empty = headers(&[("x-request-id", "")]);
        let from_empty = request_id_from(&empty);
        assert_eq!(from_empty.len(), 32, "uuid4().hex is 32 chars");
        let missing = request_id_from(&HeaderMap::new());
        assert_eq!(missing.len(), 32);
    }

    #[test]
    fn failure_classification_covers_every_arm() {
        use pidash_services::extensions::{CredentialError, CredentialUnavailable};

        let unavailable = CredentialError::Unavailable(CredentialUnavailable {
            reason: "byok_not_supported_on_desktop".to_owned(),
            message: "BYOK is not supported on the desktop app.".to_owned(),
        });
        match classify_model_token_failure(&unavailable) {
            ModelTokenFailure::Reason { reason, detail } => {
                assert_eq!(reason, "profile_unavailable");
                assert_eq!(detail, "byok_not_supported_on_desktop");
            }
            ModelTokenFailure::Transient => panic!("unavailable must pause"),
        }

        let wallet = CredentialError::WalletEmpty {
            detail: "InsufficientBalance".to_owned(),
        };
        match classify_model_token_failure(&wallet) {
            ModelTokenFailure::Reason { reason, detail } => {
                assert_eq!(reason, "wallet_empty");
                assert_eq!(detail, "InsufficientBalance");
            }
            ModelTokenFailure::Transient => panic!("wallet must pause"),
        }

        let revoked = CredentialError::Classified {
            code: "gateway_session_revoked".to_owned(),
            status: 401,
        };
        match classify_model_token_failure(&revoked) {
            ModelTokenFailure::Reason { reason, .. } => {
                assert_eq!(reason, "gateway_session_revoked")
            }
            ModelTokenFailure::Transient => panic!("revoked must pause"),
        }

        let transient = CredentialError::Classified {
            code: "gateway_unavailable".to_owned(),
            status: 503,
        };
        assert!(matches!(
            classify_model_token_failure(&transient),
            ModelTokenFailure::Transient
        ));

        assert_eq!(reason_status("wallet_empty"), StatusCode::PAYMENT_REQUIRED);
        assert_eq!(
            reason_status("gateway_session_revoked"),
            StatusCode::CONFLICT
        );
        assert_eq!(reason_status("profile_unavailable"), StatusCode::CONFLICT);
        assert_eq!(reason_status("engine_disabled"), StatusCode::CONFLICT);
    }
}
