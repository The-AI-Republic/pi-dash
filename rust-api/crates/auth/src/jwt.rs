#![forbid(unsafe_code)]

//! Runner access-token (JWT) validation (`pi_dash.runner.services.tokens`).
//!
//! Python references: `mint_access_token` / `decode_access_token` in
//! `pi_dash/runner/services/tokens.py` and the five-step verification in
//! `RunnerAccessTokenAuthentication.authenticate`
//! (`pi_dash/runner/authentication.py`, `design.md` §5.4).
//!
//! Wire format: HS256 JWT, `kid` header selecting the signing key,
//! `iss` `"pi-dash-cloud"`, claims `sub` (runner id), `uid`, `wid`
//! (trust-principal binding), `iat`, `exp`, `rtg` (refresh-token
//! generation). `decode_access_token` here covers steps 1–2 of §5.4
//! (signature by `kid`, `exp`, required-claim shape); steps 3–5 need the
//! `Runner` row (`rtg` freshness, force-refresh floor, URL runner match, live
//! revocation), so the caller performs them on the returned [`AccessClaims`].

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error as ThisError;

/// JWT algorithm for access tokens.
pub const ACCESS_TOKEN_ALG: &str = "HS256";
/// Expected issuer.
pub const ACCESS_TOKEN_ISS: &str = "pi-dash-cloud";

/// Django error codes from `AccessTokenError`, kept verbatim so callers and
/// logs match Python.
pub mod code {
    pub const MALFORMED: &str = "access_token_malformed";
    pub const UNKNOWN_KEY: &str = "access_token_unknown_key";
    pub const EXPIRED: &str = "access_token_expired";
    pub const INVALID: &str = "access_token_invalid";
}

/// Why an access token was rejected.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum JwtError {
    #[error("access_token_malformed")]
    Malformed,
    #[error("access_token_unknown_key: {0}")]
    UnknownKey(String),
    #[error("access_token_expired")]
    Expired,
    #[error("access_token_invalid: {0}")]
    Invalid(String),
}

impl JwtError {
    pub fn code(&self) -> &'static str {
        match self {
            JwtError::Malformed => code::MALFORMED,
            JwtError::UnknownKey(_) => code::UNKNOWN_KEY,
            JwtError::Expired => code::EXPIRED,
            JwtError::Invalid(_) => code::INVALID,
        }
    }
}

/// One key-ring entry: `settings.RUNNER_ACCESS_TOKEN_KEYS` rows are
/// `{kid, secret, status}`; only `active` keys mint and verify.
#[derive(Debug, Clone)]
pub struct KeyEntry {
    pub kid: String,
    pub secret: Vec<u8>,
    pub active: bool,
}

/// Verifying key ring.
#[derive(Debug, Clone, Default)]
pub struct KeyRing {
    keys: Vec<KeyEntry>,
}

impl KeyRing {
    pub fn new(keys: Vec<KeyEntry>) -> Self {
        Self { keys }
    }

    /// Dev fallback from `_key_ring()`: with no configured keys, a single
    /// `active` key `"default"` derived as
    /// `SHA256-hex("runner/access-token/" + SECRET_KEY)`.
    ///
    /// The hex string itself is the HMAC key (PyJWT receives it as a `str`,
    /// i.e. its UTF-8 bytes) — not the decoded digest.
    pub fn dev_from_secret(secret_key: impl AsRef<[u8]>) -> Self {
        let derived = hex::encode(Sha256::digest(
            ["runner/access-token/".as_bytes(), secret_key.as_ref()].concat(),
        ));
        Self {
            keys: vec![KeyEntry {
                kid: "default".to_owned(),
                secret: derived.into_bytes(),
                active: true,
            }],
        }
    }

    /// First `active` key id — what `mint_access_token` signs with. `None`
    /// mirrors Python's `RuntimeError("no active access-token signing key
    /// configured")`; there is no minter here, so callers treat it as
    /// misconfiguration.
    pub fn active_kid(&self) -> Option<&str> {
        self.keys
            .iter()
            .find(|entry| entry.active)
            .map(|entry| entry.kid.as_str())
    }

    fn secret_for(&self, kid: &str) -> Option<&[u8]> {
        self.keys
            .iter()
            .find(|entry| entry.kid == kid)
            .map(|entry| entry.secret.as_slice())
    }
}

/// Verified access-token claims (`design.md` §5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessClaims {
    pub sub: String,
    pub uid: String,
    pub wid: String,
    pub iat: u64,
    pub exp: u64,
    pub rtg: i64,
}

/// Verify signature + `exp` and return the claims.
///
/// Error mapping follows `decode_access_token`: unparseable token or missing
/// `kid` header → [`JwtError::Malformed`]; `kid` outside the ring →
/// [`JwtError::UnknownKey`]; past `exp` → [`JwtError::Expired`]; bad
/// signature or claim shape → [`JwtError::Invalid`].
pub fn decode_access_token(raw: &str, ring: &KeyRing) -> Result<AccessClaims, JwtError> {
    let header = decode_header(raw).map_err(|_| JwtError::Malformed)?;
    if header.alg != Algorithm::HS256 {
        return Err(JwtError::Invalid(format!(
            "unexpected algorithm {:?}",
            header.alg
        )));
    }
    let kid = header.kid.ok_or(JwtError::Malformed)?;
    let secret = ring
        .secret_for(&kid)
        .ok_or_else(|| JwtError::UnknownKey(kid.clone()))?;
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[ACCESS_TOKEN_ISS]);
    validation.set_required_spec_claims(&["exp", "iat", "sub"]);
    // Django passes no leeway: expiry is exact.
    validation.leeway = 0;
    let data = decode::<AccessClaims>(raw, &DecodingKey::from_secret(secret), &validation)
        .map_err(|err| {
            use jsonwebtoken::errors::ErrorKind;
            match err.kind() {
                ErrorKind::ExpiredSignature => JwtError::Expired,
                ErrorKind::InvalidIssuer
                | ErrorKind::MissingRequiredClaim(_)
                | ErrorKind::InvalidSubject
                | ErrorKind::ImmatureSignature => JwtError::Invalid(err.to_string()),
                _ => {
                    // InvalidSignature, InvalidAlgorithm, Base64, Json, ... :
                    // Python folds every other PyJWTError into access_token_invalid.
                    JwtError::Invalid(err.to_string())
                }
            }
        })?;
    Ok(data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"f05-test-secret-key-12345";

    fn ring() -> KeyRing {
        KeyRing::dev_from_secret(SECRET)
    }

    // Tokens minted by Python (PyJWT 2.8) with the derived dev key above.
    // exp 2000003600 (year 2033) so the "valid" vector stays valid.
    const JWT_VALID: &str = "eyJhbGciOiJIUzI1NiIsImtpZCI6ImRlZmF1bHQiLCJ0eXAiOiJKV1QifQ.eyJpc3MiOiJwaS1kYXNoLWNsb3VkIiwic3ViIjoiMTExMTExMTEtMjIyMi0zMzMzLTQ0NDQtNTU1NTU1NTU1NTU1IiwidWlkIjoiN2M5ZTY2NzktNzQyNS00MGRlLTk0NGItZTI5YjVlNzM3OTAzIiwid2lkIjoiYWFhYWFhYWEtYmJiYi1jY2NjLWRkZGQtZWVlZWVlZWVlZWVlIiwiaWF0IjoyMDAwMDAwMDAwLCJleHAiOjIwMDAwMDM2MDAsInJ0ZyI6N30.d13__-h-ksMCqnA09HLhIrEbbHJluA4SGv-WaJhI5NU";
    const JWT_EXPIRED: &str = "eyJhbGciOiJIUzI1NiIsImtpZCI6ImRlZmF1bHQiLCJ0eXAiOiJKV1QifQ.eyJpc3MiOiJwaS1kYXNoLWNsb3VkIiwic3ViIjoiMTExMTExMTEtMjIyMi0zMzMzLTQ0NDQtNTU1NTU1NTU1NTU1IiwidWlkIjoiN2M5ZTY2NzktNzQyNS00MGRlLTk0NGItZTI5YjVlNzM3OTAzIiwid2lkIjoiYWFhYWFhYWEtYmJiYi1jY2NjLWRkZGQtZWVlZWVlZWVlZWVlIiwiaWF0IjoxMDAwMDAwMDAwLCJleHAiOjEwMDAwMDEwMDAsInJ0ZyI6N30.6tSamaBZ1J63yJVaB15N6TbM3qppHRazqO17yIn9QB0";
    const JWT_NO_KID: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJwaS1kYXNoLWNsb3VkIiwic3ViIjoiMTExMTExMTEtMjIyMi0zMzMzLTQ0NDQtNTU1NTU1NTU1NTU1IiwidWlkIjoiN2M5ZTY2NzktNzQyNS00MGRlLTk0NGItZTI5YjVlNzM3OTAzIiwid2lkIjoiYWFhYWFhYWEtYmJiYi1jY2NjLWRkZGQtZWVlZWVlZWVlZWVlIiwiaWF0IjoyMDAwMDAwMDAwLCJleHAiOjIwMDAwMDM2MDAsInJ0ZyI6N30.GC5scG2fEbejkiaEvnffbUSjTVnbg2trOAEsZXTYD5w";
    const JWT_BAD_SIG: &str = "eyJhbGciOiJIUzI1NiIsImtpZCI6ImRlZmF1bHQiLCJ0eXAiOiJKV1QifQ.eyJpc3MiOiJwaS1kYXNoLWNsb3VkIiwic3ViIjoiMTExMTExMTEtMjIyMi0zMzMzLTQ0NDQtNTU1NTU1NTU1NTU1IiwidWlkIjoiN2M5ZTY2NzktNzQyNS00MGRlLTk0NGItZTI5YjVlNzM3OTAzIiwid2lkIjoiYWFhYWFhYWEtYmJiYi1jY2NjLWRkZGQtZWVlZWVlZWVlZWVlIiwiaWF0IjoyMDAwMDAwMDAwLCJleHAiOjIwMDAwMDM2MDAsInJ0ZyI6N30.Xy5algicfOoSC916pmijU7_UszLV-NvCkQ5_84nFrZI";
    const JWT_NO_RTG: &str = "eyJhbGciOiJIUzI1NiIsImtpZCI6ImRlZmF1bHQiLCJ0eXAiOiJKV1QifQ.eyJpc3MiOiJwaS1kYXNoLWNsb3VkIiwic3ViIjoiMTExMTExMTEtMjIyMi0zMzMzLTQ0NDQtNTU1NTU1NTU1NTU1IiwidWlkIjoiN2M5ZTY2NzktNzQyNS00MGRlLTk0NGItZTI5YjVlNzM3OTAzIiwid2lkIjoiYWFhYWFhYWEtYmJiYi1jY2NjLWRkZGQtZWVlZWVlZWVlZWVlIiwiaWF0IjoyMDAwMDAwMDAwLCJleHAiOjIwMDAwMDM2MDB9.ntAeM-XbpfZkVodjYcIo10ZpWwNyr2crMqyXa2irx6Q";

    #[test]
    fn dev_key_derivation_matches_python() {
        // Python: hashlib.sha256(("runner/access-token/" + SECRET_KEY)).hexdigest(),
        // used as the HMAC key verbatim (UTF-8 of the hex string).
        assert_eq!(
            ring().secret_for("default").unwrap(),
            b"3adc8ab2a89721ae46e257ebf3b30d1de843819ed36d02f55b8b151aa8f4c974"
        );
        assert_eq!(ring().active_kid(), Some("default"));
    }

    #[test]
    fn decodes_python_minted_token() {
        let claims = decode_access_token(JWT_VALID, &ring()).unwrap();
        assert_eq!(claims.sub, "11111111-2222-3333-4444-555555555555");
        assert_eq!(claims.uid, "7c9e6679-7425-40de-944b-e29b5e737903");
        assert_eq!(claims.wid, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        assert_eq!(claims.rtg, 7);
        assert_eq!(claims.exp, 2000003600);
    }

    #[test]
    fn error_codes_match_python() {
        assert_eq!(
            decode_access_token(JWT_EXPIRED, &ring()).unwrap_err(),
            JwtError::Expired
        );
        assert_eq!(
            decode_access_token(JWT_NO_KID, &ring()).unwrap_err(),
            JwtError::Malformed
        );
        assert!(matches!(
            decode_access_token(JWT_BAD_SIG, &ring()).unwrap_err(),
            JwtError::Invalid(_)
        ));
        assert!(matches!(
            decode_access_token(JWT_NO_RTG, &ring()).unwrap_err(),
            JwtError::Invalid(_)
        ));
        assert_eq!(
            decode_access_token("not-a-jwt", &ring()).unwrap_err(),
            JwtError::Malformed
        );
        assert_eq!(
            decode_access_token(JWT_VALID, &KeyRing::default()).unwrap_err(),
            JwtError::UnknownKey("default".to_owned())
        );
        for err in [
            JwtError::Malformed,
            JwtError::UnknownKey("k".into()),
            JwtError::Expired,
            JwtError::Invalid("x".into()),
        ] {
            assert!(!err.code().is_empty());
        }
        assert_eq!(JwtError::Expired.code(), code::EXPIRED);
    }

    #[test]
    fn wrong_issuer_is_invalid() {
        use jsonwebtoken::{encode, EncodingKey, Header};
        let ring = ring();
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some("default".to_owned());
        let claims = serde_json::json!({
            "iss": "someone-else",
            "sub": "11111111-2222-3333-4444-555555555555",
            "uid": "u",
            "wid": "w",
            "iat": 2000000000,
            "exp": 2000003600,
            "rtg": 1,
        });
        let token = encode(
            &header,
            &claims,
            &EncodingKey::from_secret(ring.secret_for("default").unwrap()),
        )
        .unwrap();
        assert!(matches!(
            decode_access_token(&token, &ring).unwrap_err(),
            JwtError::Invalid(_)
        ));
    }
}
