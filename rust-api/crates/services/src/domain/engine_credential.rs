#![forbid(unsafe_code)]

//! Engine credential domain service: issue, revoke, silent rules, derived state.
//!
//! Port of `runner/services/engine_credentials.py` (PIDESKAPP-212/213, design §4.3)
//! and the entity helpers in `runner/models.py` (`current_engine_sign_in_state`,
//! `engine_credential_payload`). This module is the only writer of
//! `engine_credential` rows and of `dev_machine.engine_sign_in_state` /
//! `engine_credential_expires_at` in the new system (each change in one
//! transaction, like Django).

use chrono::{DateTime, Utc};
use pidash_db::entity::{dev_machine, engine_credential};
use pidash_db::orm_tx::into_sqlx;
use sea_orm::{
    sea_query::Expr, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use uuid::Uuid;

pub const ENGINE_CREDENTIAL_TTL_DAYS: i64 = 14;
pub const ENGINE_CREDENTIAL_EXPIRING_WINDOW_DAYS: i64 = 3;
pub const MACHINE_TOKEN_IDLE_TTL_DAYS: i64 = 60;
pub const ENGINE_CREDENTIAL_PREFIX: &str = "ec_";
pub const SILENT_RENEWAL_REFUSED: &str = "engine_silent_renewal_refused";

pub mod engine_state {
    pub const NEVER: &str = "never";
    pub const ACTIVE: &str = "active";
    pub const EXPIRING: &str = "expiring";
    pub const EXPIRED: &str = "expired";
    pub const SIGNED_OUT: &str = "signed_out";
    pub const REVOKED: &str = "revoked";
}
pub mod revoke_reason {
    pub const SUPERSEDED: &str = "superseded";
    pub const SIGNED_OUT: &str = "signed_out";
    pub const MACHINE_REMOVED: &str = "machine_removed";
    pub const MACHINE_MERGED: &str = "machine_merged";
    pub const USER_SIGNED_OUT: &str = "user_signed_out";
    pub const ADMIN: &str = "admin";
}
pub mod auto_pause {
    pub const NONE: &str = "";
    pub const ENGINE_DISABLED: &str = "engine_disabled";
    pub const PROFILE_UNAVAILABLE: &str = "profile_unavailable";
    pub const ENGINE_SIGN_IN_REQUIRED: &str = "engine_sign_in_required";
}

fn at(now: DateTime<Utc>) -> sea_orm::prelude::DateTimeWithTimeZone {
    now.fixed_offset()
}

pub fn current_engine_sign_in_state(
    stored: &str,
    expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> String {
    if stored != engine_state::ACTIVE {
        return stored.to_owned();
    }
    match expires_at {
        None => engine_state::ACTIVE.to_owned(),
        Some(exp) if exp <= now => engine_state::EXPIRED.to_owned(),
        Some(exp)
            if exp - now <= chrono::Duration::days(ENGINE_CREDENTIAL_EXPIRING_WINDOW_DAYS) =>
        {
            engine_state::EXPIRING.to_owned()
        }
        Some(_) => engine_state::ACTIVE.to_owned(),
    }
}

pub fn silent_renewal_allowed(state: &str) -> bool {
    state == engine_state::EXPIRING || state == engine_state::EXPIRED
}

pub async fn silent_issue_allowed(
    db: &impl ConnectionTrait,
    machine: &dev_machine::Model,
    now: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let state = current_engine_sign_in_state(
        &machine.engine_sign_in_state,
        machine
            .engine_credential_expires_at
            .as_ref()
            .map(|dt| dt.with_timezone(&Utc)),
        now,
    );
    if silent_renewal_allowed(&state) {
        return Ok(true);
    }
    if state != engine_state::ACTIVE {
        return Ok(false);
    }
    let reported = machine
        .engine_state
        .get("sign_in_state")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !matches!(reported, "never" | "signed_out" | "revoked" | "expired") {
        return Ok(false);
    }
    let Some(reported_at) = machine
        .engine_state_reported_at
        .as_ref()
        .map(|dt| dt.with_timezone(&Utc))
    else {
        return Ok(false);
    };
    let live_created: Option<DateTime<Utc>> = engine_credential::Entity::find()
        .filter(engine_credential::Column::DevMachineId.eq(machine.id))
        .filter(engine_credential::Column::RevokedAt.is_null())
        .order_by_desc(engine_credential::Column::CreatedAt)
        .into_tuple::<(sea_orm::prelude::DateTimeWithTimeZone,)>()
        .one(db)
        .await
        .map_err(into_sqlx)?
        .map(|(dt,)| dt.with_timezone(&Utc));
    match live_created {
        None => Ok(true),
        Some(live) => Ok(reported_at > live),
    }
}

pub fn engine_profile_for_user_available(
    settings: &pidash_db::config::Settings,
) -> (bool, &'static str) {
    if !settings.managed_runner.enabled {
        return (false, auto_pause::ENGINE_DISABLED);
    }
    // CE (open source) never has a gateway lane the desktop can use (CE
    // BYOK is explicitly unsupported in model_provider); the cloud overlay
    // provides the OpenHub lane via a different build. In OSS, profile is
    // therefore unavailable, matching Django CE agent_model_profile_for_user.
    (false, auto_pause::PROFILE_UNAVAILABLE)
}

pub fn instance_can_serve_engine(settings: &pidash_db::config::Settings) -> bool {
    engine_profile_for_user_available(settings).0
}

#[derive(Debug, Clone)]
pub struct IssuedEngineCredential {
    pub raw: String,
    pub expires_at: DateTime<Utc>,
    pub credential_id: Uuid,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EngineCredentialError {
    pub code: String,
}
impl std::fmt::Display for EngineCredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)
    }
}
impl std::error::Error for EngineCredentialError {}

fn random_urlsafe(nbytes: usize) -> String {
    use base64::Engine as _;
    let mut buf = vec![0u8; nbytes];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&buf)
}

pub async fn issue_engine_credential(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    dev_machine_id: Uuid,
    secret_key: &[u8],
    silent: bool,
    now: DateTime<Utc>,
) -> Result<IssuedEngineCredential, EngineCredentialError> {
    let machine = dev_machine::Entity::find_by_id(dev_machine_id)
        .one(db)
        .await
        .map_err(|_| EngineCredentialError {
            code: "dev_machine_not_found".to_owned(),
        })?
        .ok_or_else(|| EngineCredentialError {
            code: "dev_machine_not_found".to_owned(),
        })?;
    if machine.owner_id != user_id {
        return Err(EngineCredentialError {
            code: "access_denied".to_owned(),
        });
    }
    if silent {
        let allowed = silent_issue_allowed(db, &machine, now)
            .await
            .unwrap_or(false);
        if !allowed {
            return Err(EngineCredentialError {
                code: SILENT_RENEWAL_REFUSED.to_owned(),
            });
        }
    }
    let now_tz = at(now);
    let expires_at = now + chrono::Duration::days(ENGINE_CREDENTIAL_TTL_DAYS);
    let _ = engine_credential::Entity::update_many()
        .col_expr(engine_credential::Column::RevokedAt, Expr::value(now_tz))
        .col_expr(
            engine_credential::Column::RevokedReason,
            Expr::value(revoke_reason::SUPERSEDED),
        )
        .filter(engine_credential::Column::DevMachineId.eq(dev_machine_id))
        .filter(engine_credential::Column::RevokedAt.is_null())
        .exec(db)
        .await;
    let raw = format!("{ENGINE_CREDENTIAL_PREFIX}{}", random_urlsafe(32));
    let token_hash = pidash_auth::token::hash_token(&raw, secret_key);
    let fingerprint = pidash_auth::token::fingerprint(&raw);
    let cred_id = Uuid::new_v4();
    let expires_tz = at(expires_at);
    engine_credential::Entity::insert(engine_credential::ActiveModel {
        id: Set(cred_id),
        user_id: Set(user_id),
        dev_machine_id: Set(dev_machine_id),
        token_hash: Set(token_hash),
        fingerprint: Set(fingerprint),
        created_at: Set(now_tz),
        expires_at: Set(expires_tz),
        last_used_at: Set(None),
        revoked_at: Set(None),
        revoked_reason: Set(String::new()),
    })
    .exec_without_returning(db)
    .await
    .map_err(|_| EngineCredentialError {
        code: "internal_error".to_owned(),
    })?;
    let _ = dev_machine::Entity::update_many()
        .col_expr(
            dev_machine::Column::EngineSignInState,
            Expr::value(engine_state::ACTIVE),
        )
        .col_expr(
            dev_machine::Column::EngineCredentialExpiresAt,
            Expr::value(expires_tz),
        )
        .col_expr(dev_machine::Column::UpdatedAt, Expr::value(now_tz))
        .filter(dev_machine::Column::Id.eq(dev_machine_id))
        .exec(db)
        .await;
    if machine.engine_auto_pause_reason == auto_pause::ENGINE_SIGN_IN_REQUIRED {
        let _ = dev_machine::Entity::update_many()
            .col_expr(
                dev_machine::Column::EngineAutoPauseReason,
                Expr::value(auto_pause::NONE),
            )
            .col_expr(dev_machine::Column::UpdatedAt, Expr::value(now_tz))
            .filter(dev_machine::Column::Id.eq(dev_machine_id))
            .exec(db)
            .await;
    }
    Ok(IssuedEngineCredential {
        raw,
        expires_at,
        credential_id: cred_id,
    })
}

pub async fn revoke_engine_credential(
    db: &impl ConnectionTrait,
    dev_machine_id: Uuid,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let state = match reason {
        revoke_reason::SIGNED_OUT | revoke_reason::USER_SIGNED_OUT => engine_state::SIGNED_OUT,
        revoke_reason::MACHINE_REMOVED | revoke_reason::MACHINE_MERGED | revoke_reason::ADMIN => {
            engine_state::REVOKED
        }
        _ => return Err(sqlx::Error::Protocol("unknown revoke reason".to_owned())),
    };
    let Some(machine) = dev_machine::Entity::find_by_id(dev_machine_id)
        .one(db)
        .await
        .map_err(into_sqlx)?
    else {
        return Ok(false);
    };
    let now_tz = at(now);
    let revoked = engine_credential::Entity::update_many()
        .col_expr(engine_credential::Column::RevokedAt, Expr::value(now_tz))
        .col_expr(
            engine_credential::Column::RevokedReason,
            Expr::value(reason),
        )
        .filter(engine_credential::Column::DevMachineId.eq(dev_machine_id))
        .filter(engine_credential::Column::RevokedAt.is_null())
        .exec(db)
        .await
        .map_err(into_sqlx)?;
    let revoked_count = revoked.rows_affected;
    let had_engine = machine.engine_sign_in_state != engine_state::NEVER || revoked_count > 0;
    let is_sign_out =
        reason == revoke_reason::SIGNED_OUT || reason == revoke_reason::USER_SIGNED_OUT;
    if !had_engine && is_sign_out {
        return Ok(false);
    }
    let _ = dev_machine::Entity::update_many()
        .col_expr(dev_machine::Column::EngineSignInState, Expr::value(state))
        .col_expr(
            dev_machine::Column::EngineCredentialExpiresAt,
            Expr::value(Option::<sea_orm::prelude::DateTimeWithTimeZone>::None),
        )
        .col_expr(dev_machine::Column::UpdatedAt, Expr::value(now_tz))
        .filter(dev_machine::Column::Id.eq(dev_machine_id))
        .exec(db)
        .await
        .map_err(into_sqlx)?;
    let is_legacy = machine.provisioning == "desktop_bundled";
    if had_engine
        && !is_legacy
        && machine.revoked_at.is_none()
        && machine.engine_auto_pause_reason.is_empty()
    {
        let _ = dev_machine::Entity::update_many()
            .col_expr(
                dev_machine::Column::EngineAutoPauseReason,
                Expr::value(auto_pause::ENGINE_SIGN_IN_REQUIRED),
            )
            .col_expr(dev_machine::Column::UpdatedAt, Expr::value(now_tz))
            .filter(dev_machine::Column::Id.eq(dev_machine_id))
            .exec(db)
            .await;
    }
    Ok(revoked_count > 0)
}

pub async fn revoke_engine_credentials_for_user(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<i64, sqlx::Error> {
    let rows: Vec<(Uuid,)> = engine_credential::Entity::find()
        .filter(engine_credential::Column::UserId.eq(user_id))
        .filter(engine_credential::Column::RevokedAt.is_null())
        .select_only()
        .column(engine_credential::Column::DevMachineId)
        .distinct()
        .into_tuple::<(Uuid,)>()
        .all(db)
        .await
        .map_err(into_sqlx)?;
    let mut count = 0;
    for (mid,) in rows {
        if revoke_engine_credential(db, mid, reason, now)
            .await
            .unwrap_or(false)
        {
            count += 1;
        }
    }
    Ok(count)
}

/// `DevMachine.has_live_engine_credential` (`models.py`).
pub fn has_live_engine_credential(
    stored: &str,
    expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    matches!(
        current_engine_sign_in_state(stored, expires_at, now).as_str(),
        engine_state::ACTIVE | engine_state::EXPIRING
    )
}

/// Default `profile.reason_code` for the `_engine_refusal` body
/// (`engine_credentials.py` OSS leg) when no BYOK-specific code applies.
pub fn engine_profile_reason_code() -> &'static str {
    "llm_config_missing"
}

/// Whether `user_id` holds a BYOK key (`agent_model_profile_for_user`
/// CE leg, `model_provider.py:110-113`): the `assistant_user_llm_config`
/// row exists and its encrypted bytes are non-empty (`has_api_key`).
pub async fn user_has_byok_key(db: &impl ConnectionTrait, user_id: Uuid) -> Result<bool, DbErr> {
    use pidash_db::entity::assistant_user_llm_config;

    let row = assistant_user_llm_config::Entity::find()
        .filter(assistant_user_llm_config::Column::UserId.eq(user_id))
        .one(db)
        .await?;
    Ok(row
        .map(|row| pidash_db::assistant::models::has_secret(&row.api_key_encrypted))
        .unwrap_or(false))
}

/// `profile.reason_code` for the `_engine_refusal` body
/// (`engine_credentials.py` via `agent_model_profile_for_user`):
/// `byok_not_supported_on_desktop` when the user holds a BYOK key,
/// else `llm_config_missing`.
pub async fn engine_profile_reason_code_for_user(
    db: &impl ConnectionTrait,
    user_id: Uuid,
) -> Result<&'static str, DbErr> {
    if user_has_byok_key(db, user_id).await? {
        Ok("byok_not_supported_on_desktop")
    } else {
        Ok(engine_profile_reason_code())
    }
}
