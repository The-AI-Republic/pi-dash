//! Automatic engine pause reasons (Django
//! `runner/services/engine_state.py`, built_engine_as_runner §8, §9;
//! port of #2910/#2912).
//!
//! `dev_machine.engine_auto_pause_reason` is set by the server from three
//! sources and cleared when they recover: the daemon's engine report in
//! the machine-session heartbeat ([`record_engine_state_report`]), the
//! model-token endpoint's own outcome ([`apply_model_token_outcome`]),
//! and the profile endpoints ([`apply_profile_outcome`], for new-path
//! machines and, through the desktop-session endpoints, legacy ones).
//!
//! Each source clears only the reasons it can disprove, so one source
//! never undoes a pause another set for a reason it cannot see. On
//! legacy desktop machines `engine_sign_in_required` never applies (§9):
//! they have no engine credential.
//!
//! Django checks the guards (`keep` / `only_from`) on the locked row.
//! SeaORM has no portable row lock, so [`set_engine_auto_pause_reason`]
//! re-checks them in the conditional `UPDATE` itself: a concurrent write
//! either commits first (our `WHERE` no longer matches, the update is a
//! no-op) or loses (our update wins). Same linearization as the locked
//! version for single-row state transitions.

use chrono::{DateTime, Utc};
use pidash_db::entity::dev_machine;
use pidash_db::orm_tx::into_sqlx;
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QuerySelect};
use serde_json::{Map, Value};
use uuid::Uuid;

use super::engine_credential::{
    SIGN_IN_STATE_EXPIRED, SIGN_IN_STATE_REVOKED, SIGN_IN_STATE_SIGNED_OUT, current_engine_sign_in_state,
    has_live_engine_credential, is_legacy_desktop, utc,
};
use super::engine_credential::{PAUSE_ENGINE_DISABLED, PAUSE_ENGINE_SIGN_IN_REQUIRED};

/// `DevMachine.engine_auto_pause_reason` value: not auto-paused.
pub const PAUSE_NONE: &str = "";
/// The engine needs an explicit sign-in again.
pub const PAUSE_GATEWAY_SESSION_REVOKED: &str = "gateway_session_revoked";
/// The instance wallet is empty; only the daemon sees the gateway 402.
pub const PAUSE_WALLET_EMPTY: &str = "wallet_empty";
/// The model profile is unavailable (the profile endpoint's to clear).
pub const PAUSE_PROFILE_UNAVAILABLE: &str = "profile_unavailable";

/// Reasons the daemon's own report may set (`_REPORTABLE`): a daemon-local
/// code such as `engine_not_installed` is stored, not applied.
const REPORTABLE: &[&str] = &[
    PAUSE_ENGINE_SIGN_IN_REQUIRED,
    PAUSE_GATEWAY_SESSION_REVOKED,
    PAUSE_WALLET_EMPTY,
];
/// Reasons a daemon reporting a working engine disproves
/// (`_CLEARED_BY_HEALTHY_REPORT`). The instance-level ones
/// (`profile_unavailable`, `engine_disabled`) are the profile endpoint's
/// to clear; the daemon re-reads the profile every 15 minutes.
const CLEARED_BY_HEALTHY_REPORT: &[&str] = &[
    PAUSE_GATEWAY_SESSION_REVOKED,
    PAUSE_WALLET_EMPTY,
    PAUSE_ENGINE_SIGN_IN_REQUIRED,
];
/// Reasons a successfully issued model token disproves
/// (`_CLEARED_BY_MODEL_TOKEN`). `wallet_empty` is not one: minting a
/// token does not touch the wallet, so only the daemon (which sees the
/// gateway's 402) can say it recovered.
const CLEARED_BY_MODEL_TOKEN: &[&str] = &[
    PAUSE_ENGINE_SIGN_IN_REQUIRED,
    PAUSE_GATEWAY_SESSION_REVOKED,
    PAUSE_PROFILE_UNAVAILABLE,
    PAUSE_ENGINE_DISABLED,
];
/// Reasons a profile read disproves (`_CLEARED_BY_PROFILE`).
const CLEARED_BY_PROFILE: &[&str] = &[PAUSE_PROFILE_UNAVAILABLE, PAUSE_ENGINE_DISABLED];
/// Daemon `engine_state.state` values meaning a working engine
/// (`_HEALTHY_STATES`).
const HEALTHY_STATES: &[&str] = &["available", "expiring"];
/// Pause reasons a daemon report never replaces (`_NEVER_REPLACED_BY_A_REPORT`):
/// the profile endpoint owns them.
const NEVER_REPLACED_BY_A_REPORT: &[&str] = &[PAUSE_ENGINE_DISABLED];
/// Report `sign_in_state` values that mean the engine is signed out
/// (`_SIGNED_OUT_STATES`).
const SIGNED_OUT_STATES: &[&str] = &[SIGN_IN_STATE_EXPIRED, SIGN_IN_STATE_SIGNED_OUT, SIGN_IN_STATE_REVOKED];
/// Stored report fields are stripped and cut to this length (`_MAX_FIELD`).
const MAX_FIELD: usize = 64;

/// Human-facing instant from a UTC clock, as the entities store it.
fn at(now: DateTime<Utc>) -> sea_orm::prelude::DateTimeWithTimeZone {
    now.fixed_offset()
}

/// The pause setter's refusal: Django raises `ValueError` on an unknown
/// reason.
#[derive(Debug)]
pub enum PauseError {
    UnknownReason(String),
    Database(sqlx::Error),
}

impl std::fmt::Display for PauseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownReason(reason) => {
                write!(f, "unknown engine auto-pause reason {reason:?}")
            }
            Self::Database(error) => write!(f, "database: {error}"),
        }
    }
}

impl std::error::Error for PauseError {}

impl PauseError {
    /// Fold the error into the credential module's `sqlx::Error` shape.
    /// An unknown reason carries no database state; it folds to the
    /// same refusal the credential callers already surface.
    pub(crate) fn into_sqlx(self) -> sqlx::Error {
        match self {
            Self::UnknownReason(_) => sqlx::Error::Protocol("unknown pause reason".to_owned()),
            Self::Database(error) => error,
        }
    }
}

/// Every value `DevMachine.engine_auto_pause_reason` may hold
/// (`EngineAutoPauseReason.values`).
fn is_known_reason(reason: &str) -> bool {
    matches!(
        reason,
        PAUSE_NONE
            | PAUSE_ENGINE_SIGN_IN_REQUIRED
            | PAUSE_GATEWAY_SESSION_REVOKED
            | PAUSE_WALLET_EMPTY
            | PAUSE_PROFILE_UNAVAILABLE
            | PAUSE_ENGINE_DISABLED
    )
}

/// Human-facing instant from a UTC clock, as the entities store it, for
/// the updated_at stamp (Django `save(update_fields=[..., "updated_at"])`).
fn stamp_now(now: DateTime<Utc>) -> sea_orm::prelude::DateTimeWithTimeZone {
    at(now)
}

/// `set_engine_auto_pause_reason(machine, reason, keep=(), only_from=None)`:
/// the seam for the heartbeat report and the model-token endpoint (§8).
/// Pausing releases the engines' queued locality pins; clearing drains
/// them (through [`on_engine_pause_changed`]). Returns True when the
/// value changed.
///
/// The conditions are checked on the row, so a caller that decided on an
/// earlier read cannot overwrite a reason set meanwhile: nothing changes
/// when the current reason equals `reason`, when it is in `keep`, or,
/// with `only_from`, when it is not in `only_from`.
pub async fn set_engine_auto_pause_reason(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
    reason: &str,
    keep: &[&str],
    only_from: Option<&[&str]>,
    now: DateTime<Utc>,
) -> Result<bool, PauseError> {
    if !is_known_reason(reason) {
        return Err(PauseError::UnknownReason(reason.to_owned()));
    }
    // was_paused is the dispatch hook's input. `engine_paused` is
    // `engine_paused_by_owner or engine_auto_pause_reason`, so both sides
    // of the flip come from the row: the pre-update reason (read just
    // before) and the post-update one. The read is best-effort: when the
    // row is gone the update matches nothing and the hook never fires.
    let before = dev_machine::Entity::find_by_id(machine_id)
        .one(db)
        .await
        .map_err(|error| PauseError::Database(into_sqlx(error)))?;
    let was_paused = before
        .as_ref()
        .map(|row| row.engine_paused_by_owner || !row.engine_auto_pause_reason.is_empty())
        .unwrap_or(false);

    let mut update = dev_machine::Entity::update_many()
        .col_expr(
            dev_machine::Column::EngineAutoPauseReason,
            Expr::value(reason),
        )
        .col_expr(dev_machine::Column::UpdatedAt, Expr::value(stamp_now(now)))
        // Nothing changes when the current reason equals `reason`.
        .filter(dev_machine::Column::EngineAutoPauseReason.ne(reason))
        // ... or when it is in `keep`.
        .filter(dev_machine::Column::EngineAutoPauseReason.is_not_in(keep.to_vec()));
    // With `only_from`, nothing changes when it is not in it.
    if let Some(allowed) = only_from {
        update = update.filter(dev_machine::Column::EngineAutoPauseReason.is_in(allowed.to_vec()));
    }
    let update = update.filter(dev_machine::Column::Id.eq(machine_id));
    let changed = update
        .exec(db)
        .await
        .map_err(|error| PauseError::Database(into_sqlx(error)))?
        .rows_affected
        > 0;
    if changed {
        let now_paused = {
            let reason_on_row = dev_machine::Entity::find_by_id(machine_id)
                .one(db)
                .await
                .map_err(|error| PauseError::Database(into_sqlx(error)))?
                .map(|row| row.engine_paused_by_owner || !row.engine_auto_pause_reason.is_empty())
                .unwrap_or(false);
            reason_on_row
        };
        on_engine_pause_changed(machine_id, was_paused, now_paused);
    }
    Ok(changed)
}

/// `on_engine_pause_changed(machine, was_paused=...)`: the dispatch side
/// effects of a machine's engine pause flipping (release locality pins on
/// pause, drain them on resume; `dispatch_rules.py:207-215+`).
///
/// Task-queue placeholder until NEWPIDASH-48's dispatch core lands: the
/// matcher side of the hook (engine runner ids, pin release/drain) does
/// not exist in Rust yet, so flipping the pause reason is the whole
/// behaviour today. When the dispatch port lands, this calls it; the call
/// site below does not change.
pub fn on_engine_pause_changed(machine_id: Uuid, was_paused: bool, now_paused: bool) {
    tracing::debug!(
        dev_machine_id = %machine_id,
        was_paused,
        now_paused,
        "engine_pause_changed (NEWPIDASH-48 dispatch hook pending)",
    );
}

/// `_apply(machine, reason, keep=())`: skip `engine_sign_in_required` on
/// legacy machines and do nothing on revoked machines; otherwise set.
async fn apply_reason(
    db: &impl ConnectionTrait,
    machine: &dev_machine::Model,
    reason: &str,
    keep: &[&str],
    now: DateTime<Utc>,
) -> Result<bool, PauseError> {
    if reason == PAUSE_ENGINE_SIGN_IN_REQUIRED && is_legacy_desktop(&machine.provisioning) {
        return Ok(false);
    }
    if machine.revoked_at.is_some() {
        return Ok(false);
    }
    set_engine_auto_pause_reason(db, machine.id, reason, keep, None, now).await
}

/// `_clear(machine, clearable)`: act only when the current reason is in
/// `clearable`. For `engine_sign_in_required` on a new-path machine the
/// server's own view wins: a daemon cannot clear a sign-in pause while
/// the server holds no live credential for the machine, re-checked on the
/// row just read. Clears with `only_from=(current,)`, not the whole
/// clearable set: only the reason this report read and disproves is
/// cleared.
async fn clear_reason(
    db: &impl ConnectionTrait,
    machine: &dev_machine::Model,
    clearable: &[&str],
    now: DateTime<Utc>,
) -> Result<bool, PauseError> {
    let current = machine.engine_auto_pause_reason.clone();
    if current.is_empty() || !clearable.contains(&current.as_str()) {
        return Ok(false);
    }
    if current == PAUSE_ENGINE_SIGN_IN_REQUIRED && !is_legacy_desktop(&machine.provisioning) {
        // Re-read: a revocation that committed after the caller's read is
        // the live view, and only a live credential lets the clear stand.
        let Some(fresh) = dev_machine::Entity::find_by_id(machine.id)
            .one(db)
            .await
            .map_err(|error| PauseError::Database(into_sqlx(error)))?
        else {
            return Ok(false);
        };
        if !has_live_engine_credential(
            &fresh.engine_sign_in_state,
            fresh.engine_credential_expires_at.map(utc),
            now,
        ) {
            return Ok(false);
        }
    }
    set_engine_auto_pause_reason(db, machine.id, PAUSE_NONE, &[], Some(&[current.as_str()]), now).await
}

/// `_clean(value)`: stringify, strip, and cut to 64 chars.
fn clean(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(other) => {
            let text = other
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| other.to_string());
            text.trim().chars().take(MAX_FIELD).collect()
        }
    }
}

/// `record_engine_state_report(machine, report)`: store the heartbeat's
/// `engine_state` and derive the automatic pause. `report` is
/// `{"state", "reason_code"?, "sign_in_state"?, "credential_expires_at"?}`;
/// `state` is one of `available`, `signed_out`, `expiring`, `unavailable`.
/// Returns the machine's pause reason after the write (Django refreshes
/// `machine.engine_auto_pause_reason` on the caller's row).
pub async fn record_engine_state_report(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
    report: &Map<String, Value>,
    now: DateTime<Utc>,
) -> Result<String, PauseError> {
    let state = clean(report.get("state"));
    let reason_code = clean(report.get("reason_code"));
    let mut stored = Map::new();
    stored.insert("state".to_owned(), Value::String(state.clone()));
    stored.insert(
        "reason_code".to_owned(),
        Value::String(reason_code.clone()),
    );
    for key in ["sign_in_state", "credential_expires_at"] {
        let value = report.get(key);
        let is_empty_optional = match value {
            None | Some(Value::Null) => true,
            Some(Value::String(text)) => text.is_empty(),
            Some(_) => false,
        };
        if !is_empty_optional {
            stored.insert(key.to_owned(), Value::String(clean(value)));
        }
    }
    dev_machine::Entity::update_many()
        .col_expr(
            dev_machine::Column::EngineState,
            Expr::value(Value::Object(stored.clone())),
        )
        .col_expr(
            dev_machine::Column::EngineStateReportedAt,
            Expr::value(at(now)),
        )
        .filter(dev_machine::Column::Id.eq(machine_id))
        .exec(db)
        .await
        .map_err(|error| PauseError::Database(into_sqlx(error)))?;

    let Some(fresh) = dev_machine::Entity::find_by_id(machine_id)
        .one(db)
        .await
        .map_err(|error| PauseError::Database(into_sqlx(error)))?
    else {
        return Ok(String::new());
    };
    if fresh.engine_auto_pause_reason == PAUSE_ENGINE_DISABLED {
        // The instance switched the engine off. That is the profile
        // endpoint's to set and clear; a daemon report never replaces it,
        // or the machine page would ask for a sign-in that cannot help.
    } else if REPORTABLE.contains(&reason_code.as_str()) {
        // `keep` is re-checked by the conditional update, since the
        // instance may switch the engine off between the read above and
        // the write.
        apply_reason(db, &fresh, &reason_code, NEVER_REPLACED_BY_A_REPORT, now).await?;
    } else if state == "signed_out"
        || stored
            .get("sign_in_state")
            .and_then(Value::as_str)
            .is_some_and(|sign_in| SIGNED_OUT_STATES.contains(&sign_in))
    {
        apply_reason(db, &fresh, PAUSE_ENGINE_SIGN_IN_REQUIRED, NEVER_REPLACED_BY_A_REPORT, now).await?;
    } else if HEALTHY_STATES.contains(&state.as_str()) && reason_code.is_empty() {
        clear_reason(db, &fresh, CLEARED_BY_HEALTHY_REPORT, now).await?;
    }
    Ok(fresh_after(db, machine_id)
        .await
        .map(|row| row.map(|machine| machine.engine_auto_pause_reason).unwrap_or_default())
        .unwrap_or_default())
}

/// Re-read the machine row after a write (Django re-reads `fresh` the
/// same way). `None` means the row is gone.
async fn fresh_after(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
) -> Result<Option<dev_machine::Model>, PauseError> {
    dev_machine::Entity::find_by_id(machine_id)
        .one(db)
        .await
        .map_err(|error| PauseError::Database(into_sqlx(error)))
}

/// `pause_engines_with_expired_credentials()`: pause the engine of every
/// new-path machine whose engine credential ran out, with
/// `engine_sign_in_required` (§4.3 "Expiry"), so pods route elsewhere
/// even while the daemon is offline. Returns the number paused.
///
/// Candidates are `active`, expired, unpaused and not revoked, excluding
/// legacy machines. Each is re-checked on the row just read: a renewal
/// that committed after the query must not be paused.
pub async fn pause_engines_with_expired_credentials(
    db: &impl ConnectionTrait,
    now: DateTime<Utc>,
) -> Result<u64, PauseError> {
    let candidates: Vec<(Uuid,)> = dev_machine::Entity::find()
        .select_only()
        .columns([dev_machine::Column::Id])
        .filter(dev_machine::Column::EngineSignInState.eq("active"))
        .filter(dev_machine::Column::EngineCredentialExpiresAt.lte(at(now)))
        .filter(dev_machine::Column::EngineAutoPauseReason.eq(PAUSE_NONE))
        .filter(dev_machine::Column::RevokedAt.is_null())
        .filter(dev_machine::Column::Provisioning.ne("desktop_bundled"))
        .into_tuple()
        .all(db)
        .await
        .map_err(|error| PauseError::Database(into_sqlx(error)))?;
    let mut paused = 0;
    for (machine_id,) in candidates {
        let Some(machine) = dev_machine::Entity::find_by_id(machine_id)
            .one(db)
            .await
            .map_err(|error| PauseError::Database(into_sqlx(error)))?
        else {
            continue;
        };
        // Re-check on the row just read (Django's locked re-check): a
        // renewal that committed after the candidate query must not be
        // paused, and the derived-state computation is the same one the
        // credential read path uses.
        let state = current_engine_sign_in_state(
            &machine.engine_sign_in_state,
            machine.engine_credential_expires_at.map(utc),
            now,
        );
        if state != "active" {
            continue;
        }
        if machine
            .engine_credential_expires_at
            .is_none_or(|expires| utc(expires) > now)
        {
            continue;
        }
        if !machine.engine_auto_pause_reason.is_empty() || machine.revoked_at.is_some() {
            continue;
        }
        if apply_reason(db, &machine, PAUSE_ENGINE_SIGN_IN_REQUIRED, &[], now).await? {
            paused += 1;
        }
    }
    Ok(paused)
}

/// `apply_model_token_outcome(machine, reason)`: map a model-token
/// outcome onto the machine (`""` = success). A reason applies; success
/// clears what it disproves (never `wallet_empty`).
pub async fn apply_model_token_outcome(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<bool, PauseError> {
    let Some(machine) = dev_machine::Entity::find_by_id(machine_id)
        .one(db)
        .await
        .map_err(|error| PauseError::Database(into_sqlx(error)))?
    else {
        return Ok(false);
    };
    if !reason.is_empty() {
        apply_reason(db, &machine, reason, &[], now).await
    } else {
        clear_reason(db, &machine, CLEARED_BY_MODEL_TOKEN, now).await
    }
}

/// `apply_profile_outcome(machine, reason)`: map a profile read onto the
/// machine: `engine_disabled` / `profile_unavailable`, or `""` when the
/// profile is available. A sharper reason already set (sign-in, wallet,
/// gateway) stays: the engine is paused either way, and that one says
/// what to fix.
pub async fn apply_profile_outcome(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<bool, PauseError> {
    let Some(machine) = dev_machine::Entity::find_by_id(machine_id)
        .one(db)
        .await
        .map_err(|error| PauseError::Database(into_sqlx(error)))?
    else {
        return Ok(false);
    };
    if !reason.is_empty() {
        if machine.engine_auto_pause_reason.is_empty()
            || CLEARED_BY_PROFILE.contains(&machine.engine_auto_pause_reason.as_str())
        {
            apply_reason(db, &machine, reason, &[], now).await
        } else {
            Ok(false)
        }
    } else {
        clear_reason(db, &machine, CLEARED_BY_PROFILE, now).await
    }
}

/// `apply_profile_outcome_to_legacy_machines(user, reason)`: the
/// desktop-session endpoints' share of §9. A legacy desktop machine takes
/// `wallet_empty` / `profile_unavailable` / `engine_disabled` from those
/// endpoints' failures. The endpoints are per user, not per machine, so
/// the outcome applies to all of the user's live legacy machines. Never
/// fails the request: each machine's failure is logged and skipped
/// (Django's try/except per machine).
pub async fn apply_profile_outcome_to_legacy_machines(
    db: &impl ConnectionTrait,
    user_id: Uuid,
    reason: &str,
    now: DateTime<Utc>,
) {
    let machines = dev_machine::Entity::find()
        .filter(dev_machine::Column::OwnerId.eq(user_id))
        .filter(dev_machine::Column::Provisioning.eq("desktop_bundled"))
        .filter(dev_machine::Column::RevokedAt.is_null())
        .all(db)
        .await;
    let Ok(machines) = machines else {
        tracing::error!(user_id = %user_id, "legacy engine pause update failed: read");
        return;
    };
    for machine in machines {
        let outcome = if matches!(
            reason,
            PAUSE_PROFILE_UNAVAILABLE | PAUSE_ENGINE_DISABLED | PAUSE_NONE
        ) {
            apply_profile_outcome(db, machine.id, reason, now).await
        } else if reason == PAUSE_WALLET_EMPTY {
            apply_reason(db, &machine, reason, &[], now).await
        } else {
            Ok(false)
        };
        if let Err(error) = outcome {
            tracing::error!(
                dev_machine_id = %machine.id,
                error = %error,
                "legacy engine pause update failed",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reason_allowlist() {
        for reason in [
            PAUSE_NONE,
            PAUSE_ENGINE_SIGN_IN_REQUIRED,
            PAUSE_GATEWAY_SESSION_REVOKED,
            PAUSE_WALLET_EMPTY,
            PAUSE_PROFILE_UNAVAILABLE,
            PAUSE_ENGINE_DISABLED,
        ] {
            assert!(is_known_reason(reason), "{reason}");
        }
        for unknown in ["engine_not_installed", "profile_pending", "never", "superseded"] {
            assert!(!is_known_reason(unknown), "{unknown}");
        }
    }

    #[test]
    fn clean_strips_and_cuts() {
        assert_eq!(clean(None), "");
        assert_eq!(clean(Some(&Value::Null)), "");
        assert_eq!(clean(Some(&Value::String("  ok  ".to_owned()))), "ok");
        let long: String = "x".repeat(100);
        assert_eq!(clean(Some(&Value::String(long))).len(), MAX_FIELD);
        // Non-string JSON stringifies (Django `str(value)`), then cuts.
        assert_eq!(clean(Some(&Value::from(42))), "42");
        assert_eq!(clean(Some(&Value::from(true))).as_str(), "true");
    }

    #[test]
    fn report_stores_optional_fields_only_when_nonempty() {
        // The store shape: state + reason_code always; the optional
        // fields only when non-empty (Django `record_engine_state_report`).
        let report = serde_json::json!({
            "state": "available",
            "reason_code": "",
            "sign_in_state": "",
            "credential_expires_at": null,
        });
        let object = report.as_object().unwrap();
        let state = clean(object.get("state"));
        let reason_code = clean(object.get("reason_code"));
        let mut stored = Map::new();
        stored.insert("state".to_owned(), Value::String(state));
        stored.insert("reason_code".to_owned(), Value::String(reason_code));
        for key in ["sign_in_state", "credential_expires_at"] {
            let value = object.get(key);
            let is_empty_optional = match value {
                None | Some(Value::Null) => true,
                Some(Value::String(text)) => text.is_empty(),
                Some(_) => false,
            };
            if !is_empty_optional {
                stored.insert(key.to_owned(), Value::String(clean(value)));
            }
        }
        assert_eq!(stored.len(), 2);
        assert!(stored.contains_key("state"));
        assert!(stored.contains_key("reason_code"));
    }

    #[test]
    fn healthy_report_clears_only_disprovable_reasons() {
        // A healthy daemon report disproves gateway/wallet/sign-in;
        // profile_unavailable and engine_disabled stay the profile
        // endpoint's to clear.
        for reason in CLEARED_BY_HEALTHY_REPORT {
            assert!(
                !matches!(
                    *reason,
                    PAUSE_PROFILE_UNAVAILABLE | PAUSE_ENGINE_DISABLED
                ),
                "{reason}"
            );
        }
        assert!(!CLEARED_BY_MODEL_TOKEN.contains(&PAUSE_WALLET_EMPTY));
        assert!(CLEARED_BY_MODEL_TOKEN.contains(&PAUSE_ENGINE_DISABLED));
        assert_eq!(NEVER_REPLACED_BY_A_REPORT, &[PAUSE_ENGINE_DISABLED]);
    }
}
