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
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect,
    Set,
};
use uuid::Uuid;

use pidash_db::entity::{desktop_handoff_code, dev_machine, machine_token, users};
use pidash_db::orm_tx::into_sqlx;

pub use pidash_db::entity::desktop_handoff_code as code_entity;

/// Single-use codes live 60 seconds (design §5).
pub const HANDOFF_TTL_SECS: i64 = 60;
/// Rate limit: issues + redeems per machine per minute, counted in the
/// database (codes in the last minute), so no cache is needed.
pub const HANDOFF_RATE_LIMIT: u64 = 30;
pub const HANDOFF_RATE_WINDOW_SECS: i64 = 60;

/// Dead codes (expired or consumed) are deleted on the next issue once
/// they are this old: far past the 60-second TTL and the rate window, so
/// the rate-limit counts (codes created / consumed in the last minute) are
/// untouched.
pub const HANDOFF_RETENTION_SECS: i64 = 3600;

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

/// Constant-time equality of two byte strings (the challenge compare): the
/// time taken depends only on the lengths, never on where they differ.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    a.ct_eq(b).into()
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

async fn machine_token_of(
    db: &impl ConnectionTrait,
    token_id: Uuid,
) -> Result<Option<machine_token::Model>, HandoffDbError> {
    machine_token::Entity::find_by_id(token_id)
        .one(db)
        .await
        .map_err(|e| HandoffDbError::Db(into_sqlx(e)))
}

/// Delete `user_id`'s expired or consumed codes older than the retention.
async fn prune_dead_codes(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), HandoffDbError> {
    desktop_handoff_code::Entity::delete_many()
        .filter(desktop_handoff_code::Column::UserId.eq(user_id))
        .filter(
            desktop_handoff_code::Column::CreatedAt
                .lt(at(now - chrono::Duration::seconds(HANDOFF_RETENTION_SECS))),
        )
        .filter(
            Condition::any()
                .add(desktop_handoff_code::Column::ConsumedAt.is_not_null())
                .add(desktop_handoff_code::Column::ExpiresAt.lte(at(now))),
        )
        .exec(db)
        .await
        .map_err(|e| HandoffDbError::Db(into_sqlx(e)))?;
    Ok(())
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
/// `machine_token_id` is the daemon credential that asked: the code is
/// refused on redeem once it is revoked or expired (design §5.1).
/// `enabled` is the kill switch (`DESKTOP_SESSION_HANDOFF_ENABLED`,
/// default on). Raises a refusal on the switch or the rate limit. Dead
/// codes of `user_id` past the retention are pruned here.
#[allow(clippy::too_many_arguments)]
pub async fn issue_handoff(
    db: &impl ConnectionTrait,
    secret_key: &[u8],
    user_id: Uuid,
    machine_id: Uuid,
    machine_token_id: Uuid,
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
    prune_dead_codes(db, user_id, now).await?;
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
        machine_token_id: Set(Some(machine_token_id)),
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

/// A daemon credential is live while unrevoked and unexpired
/// (`MachineToken.is_expired`: a null `expires_at` never expires).
fn token_is_live(token: &machine_token::Model, now: DateTime<Utc>) -> bool {
    token.revoked_at.is_none() && token.expires_at.is_none_or(|expires| utc(expires) > now)
}

/// A redeemed code: the caller creates the web session.
#[derive(Debug)]
pub struct RedeemedHandoff {
    pub user_id: Uuid,
    pub email: String,
}

/// Consume `code` and, when every check passes, redeem it (design §5 step
/// 6). The first redeem attempt uses the code up, pass or fail; only then
/// does the session get created. `db` must be the caller's transaction:
/// the row is read `FOR UPDATE`, so a concurrent second redeem waits for
/// the first to commit and then sees `consumed_at` and refuses; the
/// conditional consume (`consumed_at IS NULL`) must also hit exactly one
/// row, else the code is refused as already used.
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
        .lock_exclusive()
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
    let consumed = desktop_handoff_code::Entity::update_many()
        .col_expr(
            desktop_handoff_code::Column::ConsumedAt,
            sea_orm::sea_query::Expr::value(at(now)),
        )
        .filter(desktop_handoff_code::Column::Id.eq(row.id))
        .filter(desktop_handoff_code::Column::ConsumedAt.is_null())
        .exec(db)
        .await
        .map_err(into_sqlx)?;
    if consumed.rows_affected != 1 {
        // Another redeem consumed it first: single-use.
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
    if !constant_time_eq(
        challenge_for_verifier(verifier).as_bytes(),
        row.challenge.as_bytes(),
    ) {
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
    // The daemon credential the code was issued with must still be live
    // (design §5.1): same refusal as a revoked machine.
    let token = match row.machine_token_id {
        Some(token_id) => machine_token_of(db, token_id).await?,
        None => None,
    };
    if !token
        .as_ref()
        .is_some_and(|token| token_is_live(token, now))
    {
        tracing::info!(
            user_id = %row.user_id,
            machine_id = %row.dev_machine_id,
            reason = "machine_token_revoked",
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

    #[test]
    fn constant_time_eq_compares_bytes() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }

    /// Seed a user, a workspace, a machine and its daemon credential
    /// (the `read::test_support` user/workspace rows, nothing more).
    /// Returns `(user_id, machine_id, token_id)`.
    async fn seed_machine(pool: &sqlx::PgPool) -> (Uuid, Uuid, Uuid) {
        let user_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let username = format!("handoff-{}", user_id.simple());
        sqlx::query(
            "INSERT INTO users (id, password, username, email, first_name, last_name, avatar,\
             date_joined, created_at, updated_at, last_location, created_location,\
             is_superuser, is_managed, is_password_expired, is_active, is_staff,\
             is_email_verified, is_password_autoset, token, user_timezone,\
             last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,\
             is_bot, display_name, is_email_valid, is_password_reset_required)\
             VALUES ($1,'pbkdf2_sha256$600000$x$y',$2,$3,'Seed','User','',\
             now(), now(), now(), '','', false,false,false,true,false,false,false,'','UTC',\
             '','','email','',false,'Seed User',false,false)",
        )
        .bind(user_id)
        .bind(&username)
        .bind(format!("{username}@example.com"))
        .execute(pool)
        .await
        .expect("seed user");
        sqlx::query(
            "INSERT INTO workspaces (id, name, slug, owner_id, timezone, background_color,\
             created_at, updated_at) VALUES ($1,'Seed WS',$2,$3,'UTC','#ffffff', now(), now())",
        )
        .bind(workspace_id)
        .bind(format!("ws-{}", workspace_id.simple()))
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed workspace");
        let machine_id = Uuid::new_v4();
        let token_id = Uuid::new_v4();
        let host = format!("handoff-{}", machine_id.simple());
        sqlx::query(
            "INSERT INTO dev_machine (id, owner_id, host_label, label, visibility, provisioning,\
             engine_paused_by_owner, engine_auto_pause_reason, engine_team_runs,\
             engine_sign_in_state, engine_state, merge_result, created_at, updated_at)\
             VALUES ($1,$2,$3,'Handoff',0,'',false,'',false,'signed_out','{}'::jsonb,\
             '{}'::jsonb, now(), now())",
        )
        .bind(machine_id)
        .bind(user_id)
        .bind(&host)
        .execute(pool)
        .await
        .expect("seed dev_machine");
        sqlx::query(
            "INSERT INTO machine_token (id, user_id, workspace_id, dev_machine_id, host_label,\
             token_hash, token_fingerprint, label, is_service, created_at, expires_at)\
             VALUES ($1,$2,$3,$4,$5,$6,'fp','',true, now(), now() + interval '60 days')",
        )
        .bind(token_id)
        .bind(user_id)
        .bind(workspace_id)
        .bind(machine_id)
        .bind(&host)
        .bind(format!("hash-{}", token_id.simple()))
        .execute(pool)
        .await
        .expect("seed machine_token");
        (user_id, machine_id, token_id)
    }

    /// One redeem in its own transaction, committed whatever the outcome
    /// (what the endpoint does).
    async fn redeem_in_tx(
        pool: &sqlx::PgPool,
        code: &str,
        verifier: &[u8],
        user_id: Uuid,
    ) -> Result<Uuid, String> {
        let mut tx = pool.begin().await.expect("begin");
        let result = {
            let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
            redeem_handoff(&conn, SECRET, code, verifier, user_id, Utc::now()).await
        };
        tx.commit().await.expect("commit");
        match result {
            Ok(redeemed) => Ok(redeemed.user_id),
            Err(HandoffDbError::Refusal(refusal)) => Err(refusal.code),
            Err(HandoffDbError::Db(error)) => panic!("db error: {error}"),
        }
    }

    const SECRET: &[u8] = b"handoff-live-test-secret";

    /// Live DB (skips without `DATABASE_URL`): two concurrent redeems of
    /// one code — exactly one wins (the row lock + the conditional
    /// consume); a code whose daemon credential was revoked is refused.
    #[tokio::test]
    async fn live_redeem_is_single_use_and_needs_a_live_daemon_credential() {
        let Some(pool) = crate::read::test_support::pool().await else {
            return;
        };
        let (user_id, machine_id, token_id) = seed_machine(&pool).await;
        let db = pidash_db::pool::sea_orm_connection(&pool);
        let verifier = [7u8; 32];
        let challenge = challenge_for_verifier(&verifier);

        let issued = issue_handoff(
            &db,
            SECRET,
            user_id,
            machine_id,
            token_id,
            &challenge,
            true,
            Utc::now(),
        )
        .await
        .expect("issue");
        let (first, second) = tokio::join!(
            redeem_in_tx(&pool, &issued.raw, &verifier, user_id),
            redeem_in_tx(&pool, &issued.raw, &verifier, user_id),
        );
        let wins = [&first, &second].iter().filter(|r| r.is_ok()).count();
        assert_eq!(wins, 1, "exactly one redeem wins: {first:?} {second:?}");
        assert!(
            [&first, &second]
                .iter()
                .any(|r| r.as_ref().err().map(String::as_str) == Some("invalid_code")),
            "the loser is refused as already used: {first:?} {second:?}"
        );

        let issued = issue_handoff(
            &db,
            SECRET,
            user_id,
            machine_id,
            token_id,
            &challenge,
            true,
            Utc::now(),
        )
        .await
        .expect("issue");
        machine_token::Entity::update_many()
            .col_expr(
                machine_token::Column::RevokedAt,
                sea_orm::sea_query::Expr::value(at(Utc::now())),
            )
            .filter(machine_token::Column::Id.eq(token_id))
            .exec(&db)
            .await
            .expect("revoke token");
        assert_eq!(
            redeem_in_tx(&pool, &issued.raw, &verifier, user_id).await,
            Err("invalid_grant".to_owned()),
            "a revoked daemon credential refuses its codes"
        );
    }
}
