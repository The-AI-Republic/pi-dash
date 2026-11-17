//! Desktop session handoff (desktop_unified_signin §5): a CLI sign-in signs
//! in the desktop app through one single-use code.
//!
//! Mirrors `runner/services/desktop_handoff.py` (the OSS server):
//! - issue: the daemon asks with a workspace credential and a `challenge`
//!   (base64url of SHA-256(verifier)); the server stores only the code's
//!   hash, bound to the user, the machine and the challenge, valid 60s.
//! - redeem: the app redeems with the verifier — the code alone is useless.
//!   On any mismatch the code is consumed and no session is created.
//!
//! Every issue and redeem is audited with user, machine and outcome — never
//! the code. Issue and redeem are rate-limited per machine.

use base64::Engine as _;
use chrono::{DateTime, Utc};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};
use uuid::Uuid;

use pidash_db::entity::{desktop_handoff_code, dev_machine, users};
use pidash_db::orm_tx::into_sqlx;

pub use pidash_db::entity::desktop_handoff_code as code_entity;

/// Single-use codes live 60 seconds (design §5).
pub const HANDOFF_TTL_SECS: i64 = 60;
/// Rate limit: issues + redeems per machine per minute, counted in the
/// database (codes in the last minute), so no cache is needed.
pub const HANDOFF_RATE_LIMIT: u64 = 30;
pub const HANDOFF_RATE_WINDOW_SECS: i64 = 60;

/// Kill-switch value when off (design §5.1).
pub const HANDOFF_DISABLED: &str = "handoff_disabled";

fn at(now: DateTime<Utc>) -> sea_orm::prelude::DateTimeWithTimeZone {
    now.fixed_offset()
}

fn utc(value: sea_orm::prelude::DateTimeWithTimeZone) -> DateTime<Utc> {
    value.with_timezone(&Utc)
}

/// The credential prefix: the raw code is `dh_` + urlsafe, verified by the
/// server recomputing.
pub const HANDOFF_CODE_PREFIX: &str = "dh_";

fn mint_raw() -> String {
    let bytes: [u8; 32] = rand::random();
    format!(
        "{HANDOFF_CODE_PREFIX}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

/// base64url(SHA-256(verifier)) — what the app sends as `challenge` and the
/// server recomputes on redeem.
pub fn challenge_for_verifier(verifier: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier))
}

fn hash_code(raw: &str, secret_key: &[u8]) -> String {
    pidash_auth::token::hash_token(raw, secret_key)
}

fn fingerprint_of(raw: &str) -> String {
    pidash_auth::token::fingerprint(raw)
}

/// A handoff refusal; `code` is the wire code, `http_status` the reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffError {
    pub code: String,
    pub http_status: u16,
}

impl HandoffError {
    fn new(code: &str, http_status: u16) -> Self {
        Self {
            code: code.to_owned(),
            http_status,
        }
    }
}

impl std::fmt::Display for HandoffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)
    }
}
impl std::error::Error for HandoffError {}

#[derive(Debug)]
pub enum HandoffDbError {
    Db(sqlx::Error),
    Refusal(HandoffError),
}

impl From<sqlx::Error> for HandoffDbError {
    fn from(error: sqlx::Error) -> Self {
        HandoffDbError::Db(error)
    }
}

async fn user_of(
    db: &impl ConnectionTrait,
    user_id: Uuid,
) -> Result<Option<users::Model>, HandoffDbError> {
    users::Entity::find_by_id(user_id)
        .one(db)
        .await
        .map_err(|e| HandoffDbError::Db(into_sqlx(e)))
}

async fn machine_of(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
) -> Result<Option<dev_machine::Model>, HandoffDbError> {
    dev_machine::Entity::find_by_id(machine_id)
        .one(db)
        .await
        .map_err(|e| HandoffDbError::Db(into_sqlx(e)))
}

/// Codes issued for `machine_id` in the last rate window (the limit).
async fn recent_codes(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
    now: DateTime<Utc>,
) -> Result<u64, HandoffDbError> {
    desktop_handoff_code::Entity::find()
        .filter(desktop_handoff_code::Column::DevMachineId.eq(machine_id))
        .filter(
            desktop_handoff_code::Column::CreatedAt
                .gt(at(now - chrono::Duration::seconds(HANDOFF_RATE_WINDOW_SECS))),
        )
        .count(db)
        .await
        .map_err(|e| HandoffDbError::Db(into_sqlx(e)))
}

async fn recent_redeems(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
    now: DateTime<Utc>,
) -> Result<u64, HandoffDbError> {
    desktop_handoff_code::Entity::find()
        .filter(desktop_handoff_code::Column::DevMachineId.eq(machine_id))
        .filter(desktop_handoff_code::Column::ConsumedAt.is_not_null())
        .filter(
            desktop_handoff_code::Column::ConsumedAt
                .gt(at(now - chrono::Duration::seconds(HANDOFF_RATE_WINDOW_SECS))),
        )
        .count(db)
        .await
        .map_err(|e| HandoffDbError::Db(into_sqlx(e)))
}

/// An issued code: the raw value is returned once, never stored again.
#[derive(Debug)]
pub struct IssuedHandoff {
    pub raw: String,
    pub user_id: Uuid,
    pub email: String,
}

/// Issue a single-use code for `machine_id` bound to `user_id`.
///
/// `enabled` is the kill switch (`DESKTOP_SESSION_HANDOFF_ENABLED`,
/// default on). Raises a refusal on the switch or the rate limit.
pub async fn issue_handoff(
    db: &impl ConnectionTrait,
    secret_key: &[u8],
    user_id: Uuid,
    machine_id: Uuid,
    challenge: &str,
    enabled: bool,
    now: DateTime<Utc>,
) -> Result<IssuedHandoff, HandoffDbError> {
    if !enabled {
        tracing::info!(
            user_id = %user_id,
            machine_id = %machine_id,
            reason = HANDOFF_DISABLED,
            "handoff.issue_refused",
        );
        return Err(HandoffDbError::Refusal(HandoffError::new(
            HANDOFF_DISABLED,
            410,
        )));
    }
    if recent_codes(db, machine_id, now).await? >= HANDOFF_RATE_LIMIT {
        tracing::info!(
            user_id = %user_id,
            machine_id = %machine_id,
            reason = "rate_limited",
            "handoff.issue_refused",
        );
        return Err(HandoffDbError::Refusal(HandoffError::new(
            "rate_limited",
            429,
        )));
    }
    let user = user_of(db, user_id)
        .await?
        .ok_or_else(|| HandoffDbError::Refusal(HandoffError::new("invalid_grant", 400)))?;
    if !user.is_active {
        return Err(HandoffDbError::Refusal(HandoffError::new(
            "invalid_grant",
            400,
        )));
    }
    let machine = machine_of(db, machine_id)
        .await?
        .ok_or_else(|| HandoffDbError::Refusal(HandoffError::new("invalid_grant", 400)))?;
    if machine.revoked_at.is_some() || machine.owner_id != user_id {
        return Err(HandoffDbError::Refusal(HandoffError::new(
            "invalid_grant",
            400,
        )));
    }
    let raw = mint_raw();
    let expires = now + chrono::Duration::seconds(HANDOFF_TTL_SECS);
    desktop_handoff_code::Entity::insert(desktop_handoff_code::ActiveModel {
        id: Set(Uuid::new_v4()),
        code_hash: Set(hash_code(&raw, secret_key)),
        fingerprint: Set(fingerprint_of(&raw)),
        challenge: Set(challenge.to_owned()),
        created_at: Set(at(now)),
        expires_at: Set(at(expires)),
        consumed_at: Set(None),
        dev_machine_id: Set(machine_id),
        user_id: Set(user_id),
    })
    .exec_without_returning(db)
    .await
    .map_err(into_sqlx)?;
    tracing::info!(user_id = %user_id, machine_id = %machine_id, "handoff.issued");
    Ok(IssuedHandoff {
        raw,
        user_id,
        email: user.email.clone().unwrap_or_default(),
    })
}

/// A redeemed code: the caller creates the web session.
#[derive(Debug)]
pub struct RedeemedHandoff {
    pub user_id: Uuid,
    pub email: String,
}

/// Consume `code` and, when every check passes, redeem it (design §5 step
/// 6). The first redeem attempt uses the code up, pass or fail; only then
/// does the session get created. The consumer (`conn`) is expected to run
/// in the caller's transaction so the consume is atomic with the checks
/// (a concurrent second redeem sees `consumed_at` and refuses).
///
/// Checks, in order: the code is live and unused; SHA-256(verifier) matches
/// the stored challenge; the code's user is `expected_user_id`; the user
/// and machine are active.
pub async fn redeem_handoff(
    db: &impl ConnectionTrait,
    secret_key: &[u8],
    code: &str,
    verifier: &[u8],
    expected_user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<RedeemedHandoff, HandoffDbError> {
    let row = desktop_handoff_code::Entity::find()
        .filter(desktop_handoff_code::Column::CodeHash.eq(hash_code(code, secret_key)))
        .one(db)
        .await
        .map_err(into_sqlx)?;
    let Some(row) = row else {
        tracing::info!(reason = "unknown_code", "handoff.redeem_refused");
        return Err(HandoffDbError::Refusal(HandoffError::new(
            "invalid_code",
            400,
        )));
    };
    if row.consumed_at.is_some() {
        tracing::info!(
            user_id = %row.user_id,
            machine_id = %row.dev_machine_id,
            reason = "already_consumed",
            "handoff.redeem_refused",
        );
        return Err(HandoffDbError::Refusal(HandoffError::new(
            "invalid_code",
            400,
        )));
    }
    if utc(row.expires_at) <= now {
        tracing::info!(
            user_id = %row.user_id,
            machine_id = %row.dev_machine_id,
            reason = "expired",
            "handoff.redeem_refused",
        );
        return Err(HandoffDbError::Refusal(HandoffError::new(
            "expired_code",
            410,
        )));
    }
    if recent_redeems(db, row.dev_machine_id, now).await? >= HANDOFF_RATE_LIMIT {
        tracing::info!(
            user_id = %row.user_id,
            machine_id = %row.dev_machine_id,
            reason = "rate_limited",
            "handoff.redeem_refused",
        );
        return Err(HandoffDbError::Refusal(HandoffError::new(
            "rate_limited",
            429,
        )));
    }
    // Consume before the remaining checks so a leaked code is single-use
    // (design §5 step 6). Callers hold the transaction; the caller commits
    // the consume together with a successful session stamping, and rolls
    // back on server error — a mismatch commits the consume by returning
    // the refusal *after* the write (the caller must not roll back).
    desktop_handoff_code::Entity::update_many()
        .col_expr(
            desktop_handoff_code::Column::ConsumedAt,
            sea_orm::sea_query::Expr::value(at(now)),
        )
        .filter(desktop_handoff_code::Column::Id.eq(row.id))
        .filter(desktop_handoff_code::Column::ConsumedAt.is_null())
        .exec(db)
        .await
        .map_err(into_sqlx)?;
    if challenge_for_verifier(verifier) != row.challenge {
        tracing::info!(
            user_id = %row.user_id,
            machine_id = %row.dev_machine_id,
            reason = "verifier_mismatch",
            "handoff.redeem_refused",
        );
        return Err(HandoffDbError::Refusal(HandoffError::new(
            "invalid_grant",
            400,
        )));
    }
    if row.user_id != expected_user_id {
        tracing::info!(
            user_id = %row.user_id,
            machine_id = %row.dev_machine_id,
            reason = "user_mismatch",
            "handoff.redeem_refused",
        );
        return Err(HandoffDbError::Refusal(HandoffError::new(
            "invalid_grant",
            400,
        )));
    }
    let user = user_of(db, row.user_id)
        .await?
        .ok_or_else(|| HandoffDbError::Refusal(HandoffError::new("invalid_grant", 400)))?;
    if !user.is_active {
        tracing::info!(
            user_id = %row.user_id,
            machine_id = %row.dev_machine_id,
            reason = "user_inactive",
            "handoff.redeem_refused",
        );
        return Err(HandoffDbError::Refusal(HandoffError::new(
            "invalid_grant",
            400,
        )));
    }
    let machine = machine_of(db, row.dev_machine_id)
        .await?
        .ok_or_else(|| HandoffDbError::Refusal(HandoffError::new("invalid_grant", 400)))?;
    if machine.revoked_at.is_some() || machine.owner_id != row.user_id {
        tracing::info!(
            user_id = %row.user_id,
            machine_id = %row.dev_machine_id,
            reason = "machine_revoked",
            "handoff.redeem_refused",
        );
        return Err(HandoffDbError::Refusal(HandoffError::new(
            "invalid_grant",
            400,
        )));
    }
    tracing::info!(
        user_id = %row.user_id,
        machine_id = %row.dev_machine_id,
        "handoff.redeemed",
    );
    Ok(RedeemedHandoff {
        user_id: user.id,
        email: user.email.clone().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_is_base64url_of_the_digest() {
        let verifier = b"app-verifier-bytes";
        let challenge = challenge_for_verifier(verifier);
        assert_eq!(challenge.len(), 43, "base64url(SHA-256) is 43 chars");
        assert_eq!(
            challenge_for_verifier(verifier),
            challenge,
            "same verifier, same challenge"
        );
        assert_ne!(
            challenge_for_verifier(b"other"),
            challenge,
            "a different verifier challenges differently"
        );
    }
}
