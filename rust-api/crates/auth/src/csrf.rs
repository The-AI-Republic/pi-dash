#![forbid(unsafe_code)]

//! CSRF token semantics (`django.middleware.csrf` + `CSRFTokenEndpoint`).
//!
//! Python references: `django/middleware/csrf.py` and
//! `pi_dash/authentication/views/common.py::CSRFTokenEndpoint`.
//!
//! A CSRF *secret* is 32 chars from `[a-zA-Z0-9]` (the `csrftoken` cookie
//! value). What the page embeds — and what `GET get-csrf-token/` returns —
//! is the *masked* 64-char form: a fresh 32-char mask concatenated with the
//! per-character sum of secret and mask modulo 62. Unmasking subtracts the
//! mask the same way. Comparison is constant-time; format (length 32 or 64,
//! alphabet-only) is checked first, with Django's reason strings.

use rand::distr::{Alphanumeric, SampleString};
use rand::rng;
use subtle::ConstantTimeEq;
use thiserror::Error as ThisError;

/// Length of the raw secret (cookie value).
pub const CSRF_SECRET_LENGTH: usize = 32;
/// Length of the masked token (page-embedded / endpoint-returned).
pub const CSRF_TOKEN_LENGTH: usize = 64;
/// `CSRF_ALLOWED_CHARS`: ASCII alphanumerics, in Django's order.
///
/// Order matters: masking adds indices modulo 62, so reordering the alphabet
/// would produce different (unverifiable) tokens.
pub const CSRF_ALLOWED_CHARS: &[u8] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// Django's `InvalidTokenFormat` reasons, verbatim.
pub mod reason {
    pub const INCORRECT_LENGTH: &str = "has incorrect length";
    pub const INVALID_CHARACTERS: &str = "has invalid characters";
}

/// Why a token was rejected at the format gate.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum CsrfError {
    #[error("CSRF token has incorrect length")]
    IncorrectLength,
    #[error("CSRF token has invalid characters")]
    InvalidCharacters,
    #[error("CSRF secret is malformed")]
    BadSecret,
}

impl CsrfError {
    pub fn reason(&self) -> &'static str {
        match self {
            CsrfError::IncorrectLength => reason::INCORRECT_LENGTH,
            CsrfError::InvalidCharacters => reason::INVALID_CHARACTERS,
            CsrfError::BadSecret => reason::INCORRECT_LENGTH,
        }
    }
}

fn index_of(byte: u8) -> Option<usize> {
    CSRF_ALLOWED_CHARS.iter().position(|b| *b == byte)
}

/// `_get_new_csrf_string`: 32 random alphabet chars.
pub fn new_secret() -> String {
    Alphanumeric.sample_string(&mut rng(), CSRF_SECRET_LENGTH)
}

/// `_check_token_format`: length must be 32 or 64, chars alphabet-only.
pub fn check_token_format(token: &str) -> Result<(), CsrfError> {
    if token.len() != CSRF_TOKEN_LENGTH && token.len() != CSRF_SECRET_LENGTH {
        return Err(CsrfError::IncorrectLength);
    }
    if !token.bytes().all(|b| index_of(b).is_some()) {
        return Err(CsrfError::InvalidCharacters);
    }
    Ok(())
}

/// `_mask_cipher_secret` with an explicit mask (deterministic; tests use this,
/// production uses [`mask_secret`]).
pub fn mask_secret_with(secret: &str, mask: &str) -> Result<String, CsrfError> {
    if secret.len() != CSRF_SECRET_LENGTH || mask.len() != CSRF_SECRET_LENGTH {
        return Err(CsrfError::BadSecret);
    }
    if !secret.bytes().all(|b| index_of(b).is_some())
        || !mask.bytes().all(|b| index_of(b).is_some())
    {
        return Err(CsrfError::BadSecret);
    }
    let cipher: String = secret
        .bytes()
        .zip(mask.bytes())
        .map(|(s, m)| {
            let pair = (index_of(s).unwrap() + index_of(m).unwrap()) % CSRF_ALLOWED_CHARS.len();
            CSRF_ALLOWED_CHARS[pair] as char
        })
        .collect();
    Ok(format!("{mask}{cipher}"))
}

/// `_mask_cipher_secret`: mask a secret under a fresh random mask.
pub fn mask_secret(secret: &str) -> Result<String, CsrfError> {
    mask_secret_with(secret, &new_secret())
}

/// `_unmask_cipher_token`: recover the secret from a 64-char masked token.
pub fn unmask_token(token: &str) -> Result<String, CsrfError> {
    if token.len() != CSRF_TOKEN_LENGTH {
        return Err(CsrfError::IncorrectLength);
    }
    if !token.bytes().all(|b| index_of(b).is_some()) {
        return Err(CsrfError::InvalidCharacters);
    }
    let (mask, cipher) = token.split_at(CSRF_SECRET_LENGTH);
    Ok(cipher
        .bytes()
        .zip(mask.bytes())
        .map(|(c, m)| {
            // Python indexes with a possibly-negative difference; adding the
            // alphabet length first keeps the subtraction non-negative.
            let pair = (index_of(c).unwrap() + CSRF_ALLOWED_CHARS.len() - index_of(m).unwrap())
                % CSRF_ALLOWED_CHARS.len();
            CSRF_ALLOWED_CHARS[pair] as char
        })
        .collect())
}

/// `_does_token_match`: unmask a 64-char request token, then constant-time
/// compare against the cookie secret. A 32-char request token (raw secret
/// from the DOM) compares directly. Anything else cannot match.
pub fn tokens_match(request_token: &str, secret: &str) -> bool {
    let candidate = if request_token.len() == CSRF_TOKEN_LENGTH {
        match unmask_token(request_token) {
            Ok(unmasked) => unmasked,
            Err(_) => return false,
        }
    } else if request_token.len() == CSRF_SECRET_LENGTH {
        request_token.to_owned()
    } else {
        return false;
    };
    bool::from(candidate.as_bytes().ct_eq(secret.as_bytes()))
}

/// Outcome of `CSRFTokenEndpoint.get` (`get_token` semantics).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsrfTokenResponse {
    /// Secret the response cookie must carry.
    pub secret: String,
    /// Masked token for the `{"csrf_token": ...}` body.
    pub masked_token: String,
    /// Whether the response must (re)set the cookie. Django re-sends it
    /// whenever the request already carried one, to renew the expiry timer.
    pub needs_update: bool,
}

/// Mirror `get_token(request)`: reuse the cookie secret when present (and
/// flag the cookie for renewal), otherwise mint a fresh secret.
pub fn csrf_token_response(cookie_secret: Option<&str>) -> Result<CsrfTokenResponse, CsrfError> {
    match cookie_secret {
        Some(secret) => Ok(CsrfTokenResponse {
            masked_token: mask_secret(secret)?,
            secret: secret.to_owned(),
            needs_update: true,
        }),
        None => {
            let secret = new_secret();
            Ok(CsrfTokenResponse {
                masked_token: mask_secret(&secret)?,
                secret,
                needs_update: true,
            })
        }
    }
}

/// Response body for `GET get-csrf-token/`: `{"csrf_token": ...}`, HTTP 200.
pub fn csrf_token_body(masked_token: &str) -> serde_json::Value {
    serde_json::json!({"csrf_token": masked_token})
}

#[cfg(test)]
mod tests {
    use super::*;

    // Django 6.0.5 _mask_cipher_secret outputs (masks are random per call;
    // these two were captured and unmask deterministically).
    const SECRET_A: &str = "abcdefghijABCDEFGHIJ0123456789ab";
    const MASKED_A: &str = "BOOYjIa6ck8uXdlE3gBUscGqFS9VvY6GBPQ1nNgdktyVpGP9zN9ti3yjzN5StX6H";
    const SECRET_Z: &str = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";
    const MASKED_Z: &str = "tP2djhE1qoQpRlJSwcTUqJhYmpvHOvf8SerCIG3qPNfOgK8hVBijP8GnLOU6dUEx";

    #[test]
    fn unmask_matches_django() {
        assert_eq!(unmask_token(MASKED_A).unwrap(), SECRET_A);
        assert_eq!(unmask_token(MASKED_Z).unwrap(), SECRET_Z);
    }

    #[test]
    fn mask_round_trip() {
        let masked = mask_secret(SECRET_A).unwrap();
        assert_eq!(masked.len(), CSRF_TOKEN_LENGTH);
        assert_eq!(unmask_token(&masked).unwrap(), SECRET_A);
        // Every call draws a fresh mask.
        assert_ne!(
            mask_secret(SECRET_A).unwrap(),
            mask_secret(SECRET_A).unwrap()
        );
    }

    #[test]
    fn mask_subtracts_modulo_62() {
        // Secret "Z"(51) under mask "b"(1): (51+1)%62 = 52 -> '0'.
        let full_mask = "b".repeat(32);
        let masked = mask_secret_with(&"Z".repeat(32), &full_mask).unwrap();
        assert_eq!(&masked[..32], full_mask);
        assert_eq!(&masked[32..], &"0".repeat(32));
        assert_eq!(unmask_token(&masked).unwrap(), "Z".repeat(32));
    }

    #[test]
    fn match_semantics() {
        assert!(tokens_match(MASKED_A, SECRET_A));
        assert!(tokens_match(SECRET_A, SECRET_A));
        assert!(!tokens_match(MASKED_A, SECRET_Z));
        assert!(!tokens_match("short", SECRET_A));
        assert!(!tokens_match(&"x".repeat(64), SECRET_A));
        assert!(!tokens_match("", SECRET_A));
    }

    #[test]
    fn format_gate_reasons() {
        assert_eq!(
            check_token_format("short").unwrap_err().reason(),
            reason::INCORRECT_LENGTH
        );
        assert_eq!(
            check_token_format(&"!".repeat(32)).unwrap_err().reason(),
            reason::INVALID_CHARACTERS
        );
        assert!(check_token_format(SECRET_A).is_ok());
        assert!(check_token_format(MASKED_A).is_ok());
    }

    #[test]
    fn endpoint_response_reuses_cookie_secret() {
        let response = csrf_token_response(Some(SECRET_A)).unwrap();
        assert_eq!(response.secret, SECRET_A);
        assert!(response.needs_update);
        assert_eq!(unmask_token(&response.masked_token).unwrap(), SECRET_A);
        assert_eq!(
            csrf_token_body(&response.masked_token),
            serde_json::json!({"csrf_token": response.masked_token})
        );
    }

    #[test]
    fn endpoint_response_mints_without_cookie() {
        let response = csrf_token_response(None).unwrap();
        assert!(response.needs_update);
        assert_eq!(response.secret.len(), CSRF_SECRET_LENGTH);
        assert!(check_token_format(&response.secret).is_ok());
        assert_eq!(
            unmask_token(&response.masked_token).unwrap(),
            response.secret
        );
    }

    #[test]
    fn new_secret_shape() {
        let secret = new_secret();
        assert!(check_token_format(&secret).is_ok());
    }
}
