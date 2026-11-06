//! Engine credentials: the built-in engine's per-machine `ec_` credential
//! (Django `runner/services/engine_credentials.py`, built_engine_as_runner
//! §4.3; port of #2910/#2912).
//!
//! This module is the only writer of `engine_credential` rows and of the
//! two `dev_machine` sign-in fields (`engine_sign_in_state`,
//! `engine_credential_expires_at`); each change to one is made in the same
//! transaction as the other. The caller's connection carries the
//! transaction: the device flow (NEWPIDASH-51) issues the machine token in
//! the same one, so every function takes `&impl ConnectionTrait` and never
//! opens its own.
//!
//! Django serializes overlapping issue/revoke with `SELECT FOR UPDATE` on
//! the machine row. SeaORM has no portable row lock, so the Rust writes
//! re-check their predicates in the conditional `UPDATE` itself: a
//! concurrent write either commits first (our `WHERE` no longer matches,
//! the update is a no-op) or loses (our update wins). For these single-row
//! state transitions that linearizes to the same outcome as the locked
//! version, without the lock.
//!
//! The derived sign-in state (`expiring`/`expired`) is a read-time
//! computation over the stored fields, never written, exactly as Django
//! computes it in `DevMachine.current_engine_sign_in_state`.

use base64::Engine as _;
use chrono::{DateTime, Utc};
use pidash_db::entity::{dev_machine, engine_credential, users};
use pidash_db::orm_tx::into_sqlx;
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set};
use uuid::Uuid;

/// The engine credential's `ec_` prefix (`ENGINE_CREDENTIAL_PREFIX`).
pub const ENGINE_CREDENTIAL_PREFIX: &str = "ec_";

/// Fixed engine-credential lifetime from issue (`ENGINE_CREDENTIAL_TTL`,
/// 14 days). A renewal is a new issue, never an extension.
pub const ENGINE_CREDENTIAL_TTL_DAYS: i64 = 14;

/// How close to expiry the state flips to `expiring`
/// (`ENGINE_CREDENTIAL_EXPIRING_WINDOW`, 3 days).
pub const ENGINE_CREDENTIAL_EXPIRING_WINDOW_DAYS: i64 = 3;

/// Never-enabled sign-in state; a sign-out of a `never` machine stays
/// `never` (#2912) so the app keeps offering "Turn on".
pub const SIGN_IN_STATE_NEVER: &str = "never";
/// Signed in, well outside the expiring window.
pub const SIGN_IN_STATE_ACTIVE: &str = "active";
/// Derived: active and inside the expiring window.
pub const SIGN_IN_STATE_EXPIRING: &str = "expiring";
/// Derived: active and at or past expiry.
pub const SIGN_IN_STATE_EXPIRED: &str = "expired";
/// Signed out by the user (the daemon holds no credential).
pub const SIGN_IN_STATE_SIGNED_OUT: &str = "signed_out";
/// Revoked by removal, merge, or admin.
pub const SIGN_IN_STATE_REVOKED: &str = "revoked";

/// Auto-pause reason: no pause.
pub const PAUSE_NONE: &str = "";
/// Auto-pause reason: the engine needs an explicit sign-in.
pub const PAUSE_ENGINE_SIGN_IN_REQUIRED: &str = "engine_sign_in_required";
/// Auto-pause reason: the instance kill switch is off.
pub const PAUSE_ENGINE_DISABLED: &str = "engine_disabled";
/// Auto-pause reason: the model profile is unavailable.
pub const PAUSE_PROFILE_UNAVAILABLE: &str = "profile_unavailable";

/// `silent_issue_allowed`'s store arm (`SILENT_RENEWAL_ALLOWED_STATES`).
pub fn silent_renewal_allowed(state: &str) -> bool {
    matches!(state, SIGN_IN_STATE_EXPIRING | SIGN_IN_STATE_EXPIRED)
}

/// Sign-in states a daemon reports when it holds no credential of its own
/// (`_DAEMON_HOLDS_NO_CREDENTIAL`): `never`, `signed_out`, `revoked`,
/// `expired`. `active` and `expiring` mean it still holds one.
fn daemon_holds_no_credential(reported: &str) -> bool {
    matches!(
        reported,
        SIGN_IN_STATE_NEVER
            | SIGN_IN_STATE_SIGNED_OUT
            | SIGN_IN_STATE_REVOKED
            | SIGN_IN_STATE_EXPIRED
    )
}

/// Human-facing instant from a UTC clock, as the entities store it.
fn at(now: DateTime<Utc>) -> sea_orm::prelude::DateTimeWithTimeZone {
    now.fixed_offset()
}

/// Drop the entity's offset, back to a UTC instant.
pub(crate) fn utc(value: sea_orm::prelude::DateTimeWithTimeZone) -> DateTime<Utc> {
    value.with_timezone(&Utc)
}

/// `DevMachine.current_engine_sign_in_state`: the stored state, with
/// `expiring`/`expired` derived from `engine_credential_expires_at` when
/// the stored state is `active`. A missing expiry means `active`; an empty
/// stored state reads as `never` (Django `self.engine_sign_in_state or
/// NEVER`; the column has a DB default, so this is belt-and-braces).
pub fn current_engine_sign_in_state(
    stored: &str,
    expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> String {
    let stored = if stored.is_empty() {
        SIGN_IN_STATE_NEVER
    } else {
        stored
    };
    if stored != SIGN_IN_STATE_ACTIVE {
        return stored.to_owned();
    }
    let Some(expires_at) = expires_at else {
        return SIGN_IN_STATE_ACTIVE.to_owned();
    };
    if expires_at <= now {
        return SIGN_IN_STATE_EXPIRED.to_owned();
    }
    if expires_at - now <= chrono::Duration::days(ENGINE_CREDENTIAL_EXPIRING_WINDOW_DAYS) {
        return SIGN_IN_STATE_EXPIRING.to_owned();
    }
    SIGN_IN_STATE_ACTIVE.to_owned()
}

/// `DevMachine.has_live_engine_credential`: the derived state is `active`
/// or `expiring`.
pub fn has_live_engine_credential(
    stored: &str,
    expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    matches!(
        current_engine_sign_in_state(stored, expires_at, now).as_str(),
        SIGN_IN_STATE_ACTIVE | SIGN_IN_STATE_EXPIRING
    )
}

/// `DevMachine.is_legacy_desktop`: per-workspace machines from before the
/// engine moved into the CLI. They have no engine credential and never
/// take an `engine_sign_in_required` pause.
pub fn is_legacy_desktop(provisioning: &str) -> bool {
    provisioning == "desktop_bundled"
}

/// The engine fields shared by machine payloads and the device flow
/// (`engine_credential_payload`): derived state plus ISO-format expiry or
/// null. Key order matches Django (dict insertion order).
pub fn engine_credential_payload(
    stored: &str,
    expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> serde_json::Value {
    serde_json::json!({
        "engine_sign_in_state": current_engine_sign_in_state(stored, expires_at, now),
        "engine_credential_expires_at": expires_at.map(|exp| exp.to_rfc3339()),
    })
}

/// The non-interactive-approval half of `silent_issue_allowed`, on
/// already-read values: `expiring`/`expired` always; `active` when the
/// daemon's last report says it holds no credential and that report is
/// newer than the live credential's creation (or there is none).
pub fn silent_issue_allowed_decision(
    state: &str,
    daemon_sign_in_state: Option<&str>,
    reported_at: Option<DateTime<Utc>>,
    live_created_at: Option<DateTime<Utc>>,
) -> bool {
    if silent_renewal_allowed(state) {
        return true;
    }
    if state != SIGN_IN_STATE_ACTIVE {
        return false;
    }
    let Some(reported) = daemon_sign_in_state else {
        return false;
    };
    if !daemon_holds_no_credential(reported) {
        return false;
    }
    let Some(reported_at) = reported_at else {
        return false;
    };
    match live_created_at {
        None => true,
        Some(created) => reported_at > created,
    }
}

/// Revoke reason → stored machine sign-in state (`_STATE_FOR_REASON`).
/// `None` is an unknown reason: Django raises `ValueError`.
fn state_for_revoke_reason(reason: &str) -> Option<&'static str> {
    match reason {
        "signed_out" | "user_signed_out" => Some(SIGN_IN_STATE_SIGNED_OUT),
        "machine_removed" | "machine_merged" | "admin" => Some(SIGN_IN_STATE_REVOKED),
        _ => None,
    }
}

/// Sign-out reasons: a person signing out, not a removal or an admin
/// (`_SIGN_OUT_REASONS`).
fn is_sign_out_reason(reason: &str) -> bool {
    matches!(reason, "signed_out" | "user_signed_out")
}

/// What [`issue_engine_credential`] minted: the plaintext (returned once),
/// the row, and the expiry.
#[derive(Debug, Clone)]
pub struct IssuedEngineCredential {
    pub raw: String,
    pub id: Uuid,
    pub expires_at: DateTime<Utc>,
}

/// A credential failure. `code` is the wire code; `credential` carries the
/// row when the failure is about a known credential (Django
/// `EngineCredentialError.code` / `.credential`). The model-token
/// endpoint pauses the machine on a known, dead credential, so callers
/// need both halves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineCredentialError {
    pub code: String,
    pub credential: Option<AuthenticatedCredential>,
}

/// A live credential resolved from a presented value: the row's
/// identifying columns, nothing else (the plaintext is never stored or
/// logged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedCredential {
    pub id: Uuid,
    pub user_id: Uuid,
    pub dev_machine_id: Uuid,
    pub fingerprint: String,
    pub expires_at: DateTime<Utc>,
}

impl EngineCredentialError {
    fn sign_in_required() -> Self {
        Self {
            code: "engine_sign_in_required".to_owned(),
            credential: None,
        }
    }

    fn with_credential(code: &str, credential: AuthenticatedCredential) -> Self {
        Self {
            code: code.to_owned(),
            credential: Some(credential),
        }
    }
}

/// The issuance half's refusals: Django raises `ValueError` for a
/// non-owner and `EngineCredentialError` for a refused silent renewal.
#[derive(Debug)]
pub enum IssueError {
    NotMachineOwner,
    SilentRenewalRefused,
    Database(sqlx::Error),
}

impl std::fmt::Display for IssueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotMachineOwner => {
                write!(f, "engine credentials are issued only to the machine's owner")
            }
            Self::SilentRenewalRefused => write!(f, "engine_credentials silently refused"),
            Self::Database(error) => write!(f, "database: {error}"),
        }
    }
}

impl std::error::Error for IssueError {}

/// The revoke half's unknown-reason refusal (Django `ValueError`).
#[derive(Debug)]
pub enum RevokeError {
    UnknownReason(String),
    Database(sqlx::Error),
}

impl std::fmt::Display for RevokeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownReason(reason) => {
                write!(f, "unknown engine credential revoke reason {reason:?}")
            }
            Self::Database(error) => write!(f, "database: {error}"),
        }
    }
}

impl std::error::Error for RevokeError {}

/// `EngineCredentialError` as a `Result`: the authenticate half raises
/// rather than returning an enum.
#[derive(Debug)]
pub enum AuthError {
    Credential(EngineCredentialError),
    Database(sqlx::Error),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Credential(error) => write!(f, "engine_credential {}", error.code),
            Self::Database(error) => write!(f, "database: {error}"),
        }
    }
}

impl std::error::Error for AuthError {}

/// Mint `ec_` + `secrets.token_urlsafe(32)` (`_mint_raw`). Same shape as
/// the device flow's `token_urlsafe_32` (`auth_oauth/device_session.rs`);
/// that helper lives in the api crate, which services cannot import, so
/// the four-line urlsafe base64 of 32 random bytes is spelled out here.
fn mint_raw() -> String {
    let bytes: [u8; 32] = rand::random();
    format!(
        "{ENGINE_CREDENTIAL_PREFIX}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

/// Read the machine row, with the columns the credential writes check.
async fn read_machine(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
) -> Result<Option<dev_machine::Model>, sqlx::Error> {
    dev_machine::Entity::find_by_id(machine_id)
        .one(db)
        .await
        .map_err(into_sqlx)
}

/// `silent_issue_allowed(machine, now)`: the whole function, reading the
/// row itself (bridge for callers that hold only the machine id; the
/// issuer below re-checks on the row it reads for the write).
pub async fn silent_issue_allowed(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
    now: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let Some(machine) = read_machine(db, machine_id).await? else {
        return Ok(false);
    };
    silent_issue_allowed_for_row(db, &machine, now).await
}

/// `silent_issue_allowed`'s row half: derived state plus the daemon's last
/// report versus the live credential's creation.
async fn silent_issue_allowed_for_row(
    db: &impl ConnectionTrait,
    machine: &dev_machine::Model,
    now: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let state = current_engine_sign_in_state(
        &machine.engine_sign_in_state,
        machine.engine_credential_expires_at.map(utc),
        now,
    );
    let reported = machine
        .engine_state
        .get("sign_in_state")
        .and_then(serde_json::Value::as_str);
    let live_created_at = engine_credential::Entity::find()
        .filter(engine_credential::Column::DevMachineId.eq(machine.id))
        .filter(engine_credential::Column::RevokedAt.is_null())
        .order_by_desc(engine_credential::Column::CreatedAt)
        .limit(1)
        .one(db)
        .await
        .map_err(into_sqlx)?
        .map(|row| utc(row.created_at));
    Ok(silent_issue_allowed_decision(
        &state,
        reported,
        machine.engine_state_reported_at.map(utc),
        live_created_at,
    ))
}

/// `issue_engine_credential(user, dev_machine, silent)`: revoke the live
/// credential (`superseded`), mint `ec_` + urlsafe, store hash and
/// fingerprint and `now + 14d`, set the machine `active` with the new
/// expiry, and clear an `engine_sign_in_required` pause (the engine has a
/// valid credential again). The caller owns the transaction; run on a
/// `TxConn` when it is part of a larger exchange.
pub async fn issue_engine_credential(
    db: &impl ConnectionTrait,
    secret_key: &[u8],
    user_id: Uuid,
    machine_id: Uuid,
    silent: bool,
    now: DateTime<Utc>,
) -> Result<IssuedEngineCredential, IssueError> {
    let Some(machine) = read_machine(db, machine_id).await.map_err(IssueError::Database)? else {
        // Django's `select_for_update().get(...)` raises DoesNotExist on
        // an absent machine; there is no wire code to mirror, so refuse
        // like a non-owner rather than inventing one.
        return Err(IssueError::NotMachineOwner);
    };
    if machine.owner_id != user_id {
        return Err(IssueError::NotMachineOwner);
    }
    if silent
        && !silent_issue_allowed_for_row(db, &machine, now)
            .await
            .map_err(IssueError::Database)?
    {
        return Err(IssueError::SilentRenewalRefused);
    }
    let superseded = engine_credential::Entity::update_many()
        .col_expr(engine_credential::Column::RevokedAt, Expr::value(at(now)))
        .col_expr(
            engine_credential::Column::RevokedReason,
            Expr::value("superseded"),
        )
        .filter(engine_credential::Column::DevMachineId.eq(machine_id))
        .filter(engine_credential::Column::RevokedAt.is_null())
        .exec(db)
        .await
        .map_err(|error| IssueError::Database(into_sqlx(error)))?
        .rows_affected;
    let raw = mint_raw();
    let expires_at = now + chrono::Duration::days(ENGINE_CREDENTIAL_TTL_DAYS);
    let id = Uuid::new_v4();
    engine_credential::Entity::insert(engine_credential::ActiveModel {
        id: Set(id),
        user_id: Set(user_id),
        dev_machine_id: Set(machine_id),
        token_hash: Set(pidash_auth::token::hash_token(&raw, secret_key)),
        fingerprint: Set(pidash_auth::token::fingerprint(&raw)),
        created_at: Set(at(now)),
        expires_at: Set(at(expires_at)),
        last_used_at: Set(None),
        revoked_at: Set(None),
        revoked_reason: Set(String::new()),
    })
    .exec_without_returning(db)
    .await
    .map_err(|error| IssueError::Database(into_sqlx(error)))?;
    dev_machine::Entity::update_many()
        .col_expr(
            dev_machine::Column::EngineSignInState,
            Expr::value(SIGN_IN_STATE_ACTIVE),
        )
        .col_expr(
            dev_machine::Column::EngineCredentialExpiresAt,
            Expr::value(at(expires_at)),
        )
        .col_expr(dev_machine::Column::UpdatedAt, Expr::value(at(now)))
        .filter(dev_machine::Column::Id.eq(machine_id))
        .exec(db)
        .await
        .map_err(|error| IssueError::Database(into_sqlx(error)))?;
    // The engine has a valid credential again: clear the sign-in pause
    // (`set_engine_auto_pause_reason(machine, NONE)` in Django, no keep /
    // only_from: it clears whatever the row holds).
    if machine.engine_auto_pause_reason == PAUSE_ENGINE_SIGN_IN_REQUIRED {
        crate::domain::engine_state::set_engine_auto_pause_reason(db, machine_id, PAUSE_NONE, &[], None, now)
            .await
            .map_err(|error| IssueError::Database(error.into_sqlx()))?;
    }
    tracing::info!(
        user_id = %user_id,
        dev_machine_id = %machine_id,
        expires_at = %expires_at,
        superseded,
        "engine_credential.issued",
    );
    Ok(IssuedEngineCredential {
        raw,
        id,
        expires_at,
    })
}

/// `revoke_engine_credential(machine, reason)`: revoke the live
/// credential, record `signed_out`/`revoked` (and clear the expiry) even
/// when nothing is live — except on a never-enabled machine with a
/// sign-out reason, which stays `never` (#2912). On a new-path,
/// non-revoked, unpaused machine with a history of having the engine,
/// pause with `engine_sign_in_required`. Returns whether a live
/// credential was revoked; an absent machine returns `False`.
pub async fn revoke_engine_credential(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<bool, RevokeError> {
    let Some(state) = state_for_revoke_reason(reason) else {
        return Err(RevokeError::UnknownReason(reason.to_owned()));
    };
    let Some(machine) = read_machine(db, machine_id)
        .await
        .map_err(RevokeError::Database)?
    else {
        return Ok(false);
    };
    let revoked = engine_credential::Entity::update_many()
        .col_expr(engine_credential::Column::RevokedAt, Expr::value(at(now)))
        .col_expr(engine_credential::Column::RevokedReason, Expr::value(reason))
        .filter(engine_credential::Column::DevMachineId.eq(machine_id))
        .filter(engine_credential::Column::RevokedAt.is_null())
        .exec(db)
        .await
        .map_err(|error| RevokeError::Database(into_sqlx(error)))?
        .rows_affected;
    let had_engine = machine.engine_sign_in_state != SIGN_IN_STATE_NEVER || revoked > 0;
    if !had_engine && is_sign_out_reason(reason) {
        // Never turned on: a sign-out (the full `pidash auth logout`)
        // leaves it `never`, so the app still offers "Turn on".
        return Ok(false);
    }
    dev_machine::Entity::update_many()
        .col_expr(
            dev_machine::Column::EngineSignInState,
            Expr::value(state),
        )
        .col_expr(
            dev_machine::Column::EngineCredentialExpiresAt,
            Expr::value(None::<sea_orm::prelude::DateTimeWithTimeZone>),
        )
        .col_expr(dev_machine::Column::UpdatedAt, Expr::value(at(now)))
        .filter(dev_machine::Column::Id.eq(machine_id))
        .exec(db)
        .await
        .map_err(|error| RevokeError::Database(into_sqlx(error)))?;
    if had_engine
        && !is_legacy_desktop(&machine.provisioning)
        && machine.revoked_at.is_none()
        && machine.engine_auto_pause_reason.is_empty()
    {
        crate::domain::engine_state::set_engine_auto_pause_reason(
            db,
            machine_id,
            PAUSE_ENGINE_SIGN_IN_REQUIRED,
            &[],
            None,
            now,
        )
        .await
        .map_err(|error| RevokeError::Database(error.into_sqlx()))?;
    }
    tracing::info!(
        dev_machine_id = %machine_id,
        user_id = %machine.owner_id,
        reason,
        revoked,
        "engine_credential.revoked",
    );
    Ok(revoked > 0)
}

/// `revoke_engine_credentials_for_user(user, reason=user_signed_out)`:
/// revoke every machine holding one of the user's live credentials
/// (sign-out everywhere). Returns how many machines had a live credential
/// revoked.
pub async fn revoke_engine_credentials_for_user(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<u64, RevokeError> {
    let machine_ids: Vec<Uuid> = engine_credential::Entity::find()
        .filter(engine_credential::Column::UserId.eq(user_id))
        .filter(engine_credential::Column::RevokedAt.is_null())
        .select_only()
        .column(engine_credential::Column::DevMachineId)
        .into_tuple()
        .all(db)
        .await
        .map_err(|error| RevokeError::Database(into_sqlx(error)))?
        .into_iter()
        .map(|(id,)| id)
        .collect();
    let mut count = 0;
    for machine_id in machine_ids {
        if revoke_engine_credential(db, machine_id, "user_signed_out", now).await? {
            count += 1;
        }
    }
    Ok(count)
}

/// `authenticate_engine_credential(raw)`: resolve a presented `ec_`
/// value to its live credential. Unknown, expired or revoked values raise
/// `engine_sign_in_required` (the daemon's fix is the same: sign the
/// engine in again); a revoked machine or an owner mismatch raises
/// `dev_machine_revoked`; an inactive user raises
/// `engine_sign_in_required`. Bumps `last_used_at` only; never extends
/// `expires_at`.
pub async fn authenticate_engine_credential(
    db: &impl ConnectionTrait,
    secret_key: &[u8],
    raw: &str,
    now: DateTime<Utc>,
) -> Result<AuthenticatedCredential, AuthError> {
    if raw.is_empty() || !raw.starts_with(ENGINE_CREDENTIAL_PREFIX) {
        return Err(AuthError::Credential(EngineCredentialError::sign_in_required()));
    }
    let row = engine_credential::Entity::find()
        .filter(engine_credential::Column::TokenHash.eq(pidash_auth::token::hash_token(
            raw,
            secret_key,
        )))
        .one(db)
        .await
        .map_err(|error| AuthError::Database(into_sqlx(error)))?
        .ok_or_else(|| AuthError::Credential(EngineCredentialError::sign_in_required()))?;
    let credential = AuthenticatedCredential {
        id: row.id,
        user_id: row.user_id,
        dev_machine_id: row.dev_machine_id,
        fingerprint: row.fingerprint.clone(),
        expires_at: utc(row.expires_at),
    };
    if row.revoked_at.is_some() || utc(row.expires_at) <= now {
        return Err(AuthError::Credential(EngineCredentialError::with_credential(
            "engine_sign_in_required",
            credential,
        )));
    }
    let Some(machine) = read_machine(db, row.dev_machine_id)
        .await
        .map_err(AuthError::Database)?
    else {
        return Err(AuthError::Credential(EngineCredentialError::with_credential(
            "dev_machine_revoked",
            credential,
        )));
    };
    if machine.revoked_at.is_some() || machine.owner_id != row.user_id {
        return Err(AuthError::Credential(EngineCredentialError::with_credential(
            "dev_machine_revoked",
            credential,
        )));
    }
    let user = users::Entity::find_by_id(row.user_id)
        .one(db)
        .await
        .map_err(|error| AuthError::Database(into_sqlx(error)))?
        .ok_or_else(|| {
            AuthError::Credential(EngineCredentialError::with_credential(
                "engine_sign_in_required",
                credential.clone(),
            ))
        })?;
    if !user.is_active {
        return Err(AuthError::Credential(EngineCredentialError::with_credential(
            "engine_sign_in_required",
            credential,
        )));
    }
    engine_credential::Entity::update_many()
        .col_expr(engine_credential::Column::LastUsedAt, Expr::value(at(now)))
        .filter(engine_credential::Column::Id.eq(row.id))
        .exec(db)
        .await
        .map_err(|error| AuthError::Database(into_sqlx(error)))?;
    Ok(credential)
}

/// `engine_profile_for_user(user)`: `(available, pause_reason)` for
/// serving the engine to one user. `managed_enabled` is the instance kill
/// switch (`managed_runner_is_enabled`); the profile is the seam's
/// `agent_model_profile` answer. Old `engine_disabled`/`profile_unavailable`
/// names are the pause reasons the state maps to, not the wire codes.
pub fn engine_profile_for_user(
    managed_enabled: bool,
    profile: &crate::extensions::AgentModelProfile,
) -> (bool, String) {
    if !managed_enabled {
        return (false, PAUSE_ENGINE_DISABLED.to_owned());
    }
    if !profile.available {
        return (false, PAUSE_PROFILE_UNAVAILABLE.to_owned());
    }
    (true, PAUSE_NONE.to_owned())
}

/// `instance_can_serve_engine(user)`: True when `pidash auth login` should
/// issue an engine credential.
pub fn instance_can_serve_engine(
    managed_enabled: bool,
    profile: &crate::extensions::AgentModelProfile,
) -> bool {
    engine_profile_for_user(managed_enabled, profile).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at_utc(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, 10, 0, 0).unwrap()
    }

    #[test]
    fn derived_state_covers_every_arm() {
        let now = at_utc(2026, 11, 6);
        // Stored state passes through when it is not active.
        assert_eq!(
            current_engine_sign_in_state("never", Some(now), now),
            "never"
        );
        assert_eq!(
            current_engine_sign_in_state("signed_out", Some(now), now),
            "signed_out"
        );
        assert_eq!(
            current_engine_sign_in_state("revoked", None, now),
            "revoked"
        );
        // Active with no expiry is active.
        assert_eq!(
            current_engine_sign_in_state("active", None, now),
            "active"
        );
        // Active far from expiry is active.
        assert_eq!(
            current_engine_sign_in_state(
                "active",
                Some(now + chrono::Duration::days(10)),
                now
            ),
            "active"
        );
        // Inside the 3-day window is expiring (boundary: exactly 3d).
        assert_eq!(
            current_engine_sign_in_state("active", Some(now + chrono::Duration::days(3)), now),
            "expiring"
        );
        assert_eq!(
            current_engine_sign_in_state(
                "active",
                Some(now + chrono::Duration::days(2)),
                now
            ),
            "expiring"
        );
        // At or past expiry is expired.
        assert_eq!(
            current_engine_sign_in_state("active", Some(now), now),
            "expired"
        );
        assert_eq!(
            current_engine_sign_in_state("active", Some(now - chrono::Duration::seconds(1)), now),
            "expired"
        );
        // Empty stored state reads as never.
        assert_eq!(current_engine_sign_in_state("", None, now), "never");
    }

    #[test]
    fn live_and_legacy_predicates() {
        let now = at_utc(2026, 11, 6);
        assert!(has_live_engine_credential(
            "active",
            Some(now + chrono::Duration::days(10)),
            now
        ));
        assert!(has_live_engine_credential(
            "active",
            Some(now + chrono::Duration::days(2)),
            now
        ));
        assert!(!has_live_engine_credential("active", Some(now), now));
        assert!(!has_live_engine_credential("signed_out", None, now));
        assert!(is_legacy_desktop("desktop_bundled"));
        assert!(!is_legacy_desktop("manual"));
        assert!(!is_legacy_desktop(""));
    }

    #[test]
    fn payload_shape_and_key_order() {
        let now = at_utc(2026, 11, 6);
        let expires = now + chrono::Duration::days(10);
        let payload = engine_credential_payload("active", Some(expires), now);
        let object = payload.as_object().unwrap();
        let keys: Vec<&str> = object.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["engine_sign_in_state", "engine_credential_expires_at"]
        );
        assert_eq!(object["engine_sign_in_state"], "active");
        assert_eq!(
            object["engine_credential_expires_at"],
            expires.to_rfc3339()
        );
        let without = engine_credential_payload("signed_out", None, now);
        assert!(without["engine_credential_expires_at"].is_null());
    }

    #[test]
    fn silent_decision_covers_every_arm() {
        let now = at_utc(2026, 11, 6);
        let created = now - chrono::Duration::days(2);
        let reported = now - chrono::Duration::days(1);
        // expiring / expired always renew silently.
        assert!(silent_issue_allowed_decision("expiring", None, None, None));
        assert!(silent_issue_allowed_decision("expired", None, None, None));
        assert!(!silent_renewal_allowed("active"));
        assert!(!silent_renewal_allowed("never"));
        // never / signed_out / revoked never renew silently.
        for state in ["never", "signed_out", "revoked"] {
            assert!(!silent_issue_allowed_decision(
                state,
                Some("never"),
                Some(reported),
                Some(created)
            ));
        }
        // active needs a no-credential report newer than the live credential.
        assert!(silent_issue_allowed_decision(
            "active",
            Some("signed_out"),
            Some(reported),
            Some(created)
        ));
        assert!(silent_issue_allowed_decision(
            "active",
            Some("never"),
            Some(reported),
            None
        ));
        assert!(silent_issue_allowed_decision(
            "active",
            Some("expired"),
            Some(reported),
            Some(created)
        ));
        // A report older than the live credential is stale: no renewal.
        assert!(!silent_issue_allowed_decision(
            "active",
            Some("signed_out"),
            Some(created),
            Some(reported)
        ));
        // A report saying it still holds a credential: no renewal.
        for reported_state in ["active", "expiring"] {
            assert!(!silent_issue_allowed_decision(
                "active",
                Some(reported_state),
                Some(reported),
                Some(created)
            ));
        }
        // Missing report time or missing report: no renewal.
        assert!(!silent_issue_allowed_decision(
            "active",
            Some("signed_out"),
            None,
            Some(created)
        ));
        assert!(!silent_issue_allowed_decision("active", None, Some(reported), None));
    }

    #[test]
    fn revoke_reason_maps_to_state() {
        assert_eq!(state_for_revoke_reason("signed_out"), Some("signed_out"));
        assert_eq!(
            state_for_revoke_reason("user_signed_out"),
            Some("signed_out")
        );
        assert_eq!(
            state_for_revoke_reason("machine_removed"),
            Some("revoked")
        );
        assert_eq!(
            state_for_revoke_reason("machine_merged"),
            Some("revoked")
        );
        assert_eq!(state_for_revoke_reason("admin"), Some("revoked"));
        assert_eq!(state_for_revoke_reason("superseded"), None);
        assert_eq!(state_for_revoke_reason(""), None);
        assert!(is_sign_out_reason("signed_out"));
        assert!(is_sign_out_reason("user_signed_out"));
        assert!(!is_sign_out_reason("admin"));
    }

    #[test]
    fn mint_shape_matches_device_flow() {
        let raw = mint_raw();
        assert!(raw.starts_with("ec_"));
        assert_eq!(raw.len(), "ec_".len() + 43);
    }

    #[test]
    fn profile_for_user_covers_every_arm() {
        let profile = |available| crate::extensions::AgentModelProfile {
            available,
            lane: "byok".to_owned(),
            base_url: String::new(),
            model: String::new(),
            reason_code: String::new(),
            top_up_url: String::new(),
        };
        let (available, reason) = engine_profile_for_user(false, &profile(true));
        assert!(!available);
        assert_eq!(reason, "engine_disabled");
        let (available, reason) = engine_profile_for_user(true, &profile(false));
        assert!(!available);
        assert_eq!(reason, "profile_unavailable");
        let (available, reason) = engine_profile_for_user(true, &profile(true));
        assert!(available);
        assert_eq!(reason, "");
        assert!(!instance_can_serve_engine(false, &profile(true)));
        assert!(instance_can_serve_engine(true, &profile(true)));
    }
}
