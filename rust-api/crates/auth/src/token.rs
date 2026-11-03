#![forbid(unsafe_code)]

//! API-token and machine-token validation (`APIKeyAuthentication`).
//!
//! Python references:
//!
//! - `pi_dash/api/middleware/api_authentication.py` — `APIKeyAuthentication`:
//!   the `X-Api-Key` header carries either an `APIToken` (table
//!   `api_tokens`, exact match on `token`, `is_active`, unexpired) or, when
//!   it starts with `mt_`, a `MachineToken` (table `machine_token`, match on
//!   `token_hash`, unrevoked, dev-machine unrevoked, workspace member).
//! - `pi_dash/runner/services/tokens.py` — `hash_token` (HMAC-SHA256 under a
//!   pepper derived from `SECRET_KEY`) and `fingerprint` (short public id).
//!
//! Row fetching is the caller's SQL; this module holds the routing rule,
//! the key-derivation helpers, and the row predicates, so every check stays
//! testable without a database.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error as ThisError;

/// Request header carrying the token (`APIKeyAuthentication.auth_header_name`).
pub const API_KEY_HEADER: &str = "X-Api-Key";
/// Prefix routing header values to the machine-token path.
pub const MACHINE_TOKEN_PREFIX: &str = "mt_";

/// Which validator handles a presented `X-Api-Key` value.
///
/// `None` means no usable credential was presented — Django's `authenticate`
/// returns `None` there, letting other authenticators (or the permission
/// layer) decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Api,
    Machine,
}

pub fn classify_token(header_value: &str) -> Option<TokenKind> {
    if header_value.is_empty() {
        None
    } else if header_value.starts_with(MACHINE_TOKEN_PREFIX) {
        Some(TokenKind::Machine)
    } else {
        Some(TokenKind::Api)
    }
}

/// Why a presented token was rejected.
///
/// Every arm maps to Django's single `AuthenticationFailed("Given API token
/// is not valid")`; the variants exist so logs can say which predicate
/// failed without changing the wire behaviour.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum TokenError {
    #[error("given API token is not valid")]
    Invalid,
    #[error("given API token is not valid: token is revoked")]
    Revoked,
    #[error("given API token is not valid: token is expired")]
    Expired,
    #[error("given API token is not valid: token is inactive")]
    Inactive,
    #[error("given API token is not valid: caller is not a workspace member")]
    NotMember,
}

/// `hash_token`: `HMAC_SHA256(SHA256("runner/pepper/" + SECRET_KEY), raw)`,
/// hex-encoded. Stored in `machine_token.token_hash`.
pub fn hash_token(raw: &str, secret_key: impl AsRef<[u8]>) -> String {
    let pepper = Sha256::digest(["runner/pepper/".as_bytes(), secret_key.as_ref()].concat());
    let mut mac =
        Hmac::<Sha256>::new_from_slice(&pepper).expect("HMAC-SHA256 accepts any key length");
    mac.update(raw.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// `fingerprint`: first 12 hex chars of `SHA256(raw)` for logs and admin UI.
pub fn fingerprint(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))[..12].to_owned()
}

/// The `api_tokens` columns the validator reads.
#[derive(Debug, Clone)]
pub struct ApiTokenRow {
    pub token: String,
    pub is_active: bool,
    /// `expired_at` as unix seconds; `None` means "never expires".
    pub expired_at_unix: Option<i64>,
}

/// Mirror the `APIToken` lookup: exact token match, `is_active`, and
/// (`expired_at` null or strictly in the future — `expired_at__gt=now`).
pub fn validate_api_token(
    row: Option<&ApiTokenRow>,
    presented: &str,
    now_unix: i64,
) -> Result<(), TokenError> {
    let row = row.ok_or(TokenError::Invalid)?;
    if !bool::from(row.token.as_bytes().ct_eq(presented.as_bytes())) {
        return Err(TokenError::Invalid);
    }
    if !row.is_active {
        return Err(TokenError::Inactive);
    }
    if let Some(expires) = row.expired_at_unix {
        if expires <= now_unix {
            return Err(TokenError::Expired);
        }
    }
    Ok(())
}

/// The `machine_token` columns the validator reads, plus whether the linked
/// dev-machine row is revoked (`dev_machine.revoked_at IS NOT NULL`).
#[derive(Debug, Clone)]
pub struct MachineTokenRow {
    pub token_hash: String,
    pub revoked_at_unix: Option<i64>,
    pub dev_machine_revoked: bool,
}

/// Static half of the machine-token check: hash match, token unrevoked,
/// dev-machine unrevoked.
///
/// The remaining half — `is_workspace_member(user, workspace_id)` — needs a
/// DB read, so the caller performs it after this returns `Ok`. A `false`
/// there means revoke-then-deny, exactly like Python: the caller stamps
/// `revoked_at` and returns `Err(TokenError::NotMember)`.
pub fn validate_machine_token_static(
    row: Option<&MachineTokenRow>,
    presented_hash: &str,
) -> Result<(), TokenError> {
    let row = row.ok_or(TokenError::Invalid)?;
    if !bool::from(row.token_hash.as_bytes().ct_eq(presented_hash.as_bytes())) {
        return Err(TokenError::Invalid);
    }
    if row.revoked_at_unix.is_some() {
        return Err(TokenError::Revoked);
    }
    if row.dev_machine_revoked {
        return Err(TokenError::Invalid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"f05-test-secret-key-12345";

    #[test]
    fn hash_and_fingerprint_match_python() {
        // Values produced by pi_dash.runner.services.tokens with the secret above.
        assert_eq!(
            hash_token("mt_testrawtoken123", SECRET),
            "339d0691728a6af42305b4ff30115c85ad3784b235936c8a1d3cd79139c1490c"
        );
        assert_eq!(fingerprint("mt_testrawtoken123"), "515888c6b11d");
    }

    #[test]
    fn classify_routes_on_prefix() {
        assert_eq!(classify_token(""), None);
        assert_eq!(classify_token("mt_abc"), Some(TokenKind::Machine));
        assert_eq!(classify_token("pi_dash_api_deadbeef"), Some(TokenKind::Api));
    }

    fn api_row() -> ApiTokenRow {
        ApiTokenRow {
            token: "pi_dash_api_deadbeef".to_owned(),
            is_active: true,
            expired_at_unix: Some(2000000000),
        }
    }

    #[test]
    fn api_token_happy_path() {
        assert!(validate_api_token(Some(&api_row()), "pi_dash_api_deadbeef", 1999999999).is_ok());
    }

    #[test]
    fn api_token_null_expiry_never_expires() {
        let row = ApiTokenRow {
            expired_at_unix: None,
            ..api_row()
        };
        assert!(validate_api_token(Some(&row), "pi_dash_api_deadbeef", i64::MAX).is_ok());
    }

    #[test]
    fn api_token_predicates() {
        // Missing row.
        assert_eq!(
            validate_api_token(None, "pi_dash_api_deadbeef", 0).unwrap_err(),
            TokenError::Invalid
        );
        // Wrong value.
        assert_eq!(
            validate_api_token(Some(&api_row()), "pi_dash_api_other", 0).unwrap_err(),
            TokenError::Invalid
        );
        // Inactive.
        let row = ApiTokenRow {
            is_active: false,
            ..api_row()
        };
        assert_eq!(
            validate_api_token(Some(&row), "pi_dash_api_deadbeef", 0).unwrap_err(),
            TokenError::Inactive
        );
        // Expired, including the exact-second boundary (expires__gt=now).
        let row = ApiTokenRow {
            expired_at_unix: Some(1000),
            ..api_row()
        };
        assert_eq!(
            validate_api_token(Some(&row), "pi_dash_api_deadbeef", 1000).unwrap_err(),
            TokenError::Expired
        );
        assert_eq!(
            validate_api_token(Some(&row), "pi_dash_api_deadbeef", 1001).unwrap_err(),
            TokenError::Expired
        );
    }

    fn machine_row() -> MachineTokenRow {
        MachineTokenRow {
            token_hash: hash_token("mt_testrawtoken123", SECRET),
            revoked_at_unix: None,
            dev_machine_revoked: false,
        }
    }

    #[test]
    fn machine_token_happy_path() {
        let presented = hash_token("mt_testrawtoken123", SECRET);
        assert!(validate_machine_token_static(Some(&machine_row()), &presented).is_ok());
    }

    #[test]
    fn machine_token_predicates() {
        let good = hash_token("mt_testrawtoken123", SECRET);
        assert_eq!(
            validate_machine_token_static(None, &good).unwrap_err(),
            TokenError::Invalid
        );
        assert_eq!(
            validate_machine_token_static(Some(&machine_row()), "0badhash").unwrap_err(),
            TokenError::Invalid
        );
        let row = MachineTokenRow {
            revoked_at_unix: Some(1000),
            ..machine_row()
        };
        assert_eq!(
            validate_machine_token_static(Some(&row), &good).unwrap_err(),
            TokenError::Revoked
        );
        let row = MachineTokenRow {
            dev_machine_revoked: true,
            ..machine_row()
        };
        assert_eq!(
            validate_machine_token_static(Some(&row), &good).unwrap_err(),
            TokenError::Invalid
        );
    }
}
