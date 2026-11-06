#![forbid(unsafe_code)]

//! Engine credential domain helpers for NEWPIDASH-52.
//!
//! Minimal port covering what machine identity needs: the derived
//! `current_engine_sign_in_state` / `has_live_engine_credential`
//! (`models.py` `DevMachine`), `engine_profile_for_user_available`
//! (`engine_credentials.py` OSS leg: enabled → `engine_disabled`,
//! otherwise CE profile unavailable), and `revoke_engine_credential`
//! for the merge path (`machine_merged`). Full issue/revoke/silent
//! logic lives in NEWPIDASH-50/51; this module keeps N52's dependency
//! small and merges cleanly (prefer main on duplicates).

use chrono::{DateTime, Utc};
use pidash_db::entity::{dev_machine, engine_credential};
use pidash_db::orm_tx::into_sqlx;
use sea_orm::{sea_query::Expr, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

pub const ENGINE_CREDENTIAL_EXPIRING_WINDOW_DAYS: i64 = 3;

pub mod engine_state {
    pub const NEVER: &str = "never";
    pub const ACTIVE: &str = "active";
    pub const EXPIRING: &str = "expiring";
    pub const EXPIRED: &str = "expired";
    pub const SIGNED_OUT: &str = "signed_out";
    pub const REVOKED: &str = "revoked";
}

pub mod revoke_reason {
    pub const SIGNED_OUT: &str = "signed_out";
    pub const USER_SIGNED_OUT: &str = "user_signed_out";
    pub const MACHINE_REMOVED: &str = "machine_removed";
    pub const MACHINE_MERGED: &str = "machine_merged";
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

/// `DevMachine.current_engine_sign_in_state` (`models.py`).
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

/// Human-readable `profile.reason_code` for the `_engine_refusal` body
/// (`engine_credentials.py`): OSS CE has no gateway lane, so the profile
/// is always unavailable when the kill switch is on.
pub fn engine_profile_reason_code() -> &'static str {
    "llm_config_missing"
}

/// `(available, pause_reason)` for `_engine_refusal` (`engine_profile_for_user`
/// OSS leg). `reason_code` for the body comes from
/// [`engine_profile_reason_code`].
pub fn engine_profile_for_user_available(
    settings: &pidash_db::config::Settings,
) -> (bool, &'static str) {
    if !settings.managed_runner.enabled {
        return (false, auto_pause::ENGINE_DISABLED);
    }
    (false, auto_pause::PROFILE_UNAVAILABLE)
}

/// `revoke_engine_credential` for the merge path (`machine_merged`):
/// revoke the live credential, record state `revoked`, clear expiry.
/// Callers hold the transaction; drains run on commit.
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
