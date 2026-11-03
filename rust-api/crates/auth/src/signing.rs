#![forbid(unsafe_code)]

//! Django signing codec (`django.core.signing`, TimestampSigner flavour).
//!
//! Python reference: `django.core.signing.dumps` / `loads`. `dumps` signs
//! with [`TimestampSigner`], so every signed value — including the
//! `session_data` column read by [`crate::session`] — has the shape
//!
//! ```text
//! base64url(json) ["." prefix when zlib-compressed] ":" base62(unix_time) ":" base64url(signature)
//! ```
//!
//! The signature is `base64url(HMAC_SHA256(key, value))` where the HMAC key
//! is `SHA256((salt + "signer") + secret)`. That two-step derivation is
//! `django.utils.crypto.salted_hmac` with the Signer's SHA256 algorithm:
//! the digest of the salted secret becomes the HMAC key. Reproducing the
//! nesting exactly is what makes Django-issued values verify here.

use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error as ThisError;

type HmacSha256 = Hmac<Sha256>;

/// Salt namespace for the Django session rows (`pi_dash.db.models.session`).
///
/// Django derives the per-backend salt as `"django.contrib.sessions."` plus
/// the session-store class name. Pi Dash subclasses the DB store without
/// renaming it, so the salt is `...SessionStore` — not
/// `django.contrib.sessions.backends.db`.
pub const SESSION_SIGNING_SALT: &str = "django.contrib.sessions.SessionStore";

const BASE62_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Why a signed value was rejected.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum SigningError {
    #[error("signature does not match")]
    BadSignature,
    #[error("signed value is malformed")]
    Malformed,
    #[error("compressed payload does not decompress")]
    Decompress,
    #[error("payload is not valid JSON")]
    Json,
}

/// Django `Signer`/`TimestampSigner` with a fixed salt.
///
/// `secret_key` is Django's `SECRET_KEY` (UTF-8 bytes). Extra rotation keys
/// map to `SECRET_KEY_FALLBACKS`: verification tries each in order.
#[derive(Debug, Clone)]
pub struct Signer {
    keys: Vec<Vec<u8>>,
    salt: String,
}

impl Signer {
    pub fn new(secret_key: impl AsRef<[u8]>, salt: &str) -> Self {
        Self {
            keys: vec![secret_key.as_ref().to_vec()],
            salt: salt.to_owned(),
        }
    }

    pub fn with_fallbacks(secret_key: impl AsRef<[u8]>, fallbacks: &[Vec<u8>], salt: &str) -> Self {
        let mut keys = vec![secret_key.as_ref().to_vec()];
        keys.extend(fallbacks.iter().cloned());
        Self {
            keys,
            salt: salt.to_owned(),
        }
    }

    /// `Signer.signature`: `base64url(HMAC_SHA256(derived, value))`.
    fn signature(&self, value: &str, key: &[u8]) -> String {
        let derived = Sha256::digest([format!("{}signer", self.salt).as_bytes(), key].concat());
        let mut mac =
            HmacSha256::new_from_slice(&derived).expect("HMAC-SHA256 accepts any key length");
        mac.update(value.as_bytes());
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }

    /// Verify `value:signature` against every known key.
    fn verify(&self, value: &str, signature: &str) -> Result<(), SigningError> {
        let ok = self.keys.iter().any(|key| {
            let expected = self.signature(value, key);
            bool::from(expected.as_bytes().ct_eq(signature.as_bytes()))
        });
        if ok {
            Ok(())
        } else {
            Err(SigningError::BadSignature)
        }
    }

    /// `TimestampSigner.unsign` without `max_age`: return the unsigned value.
    ///
    /// Django's session decoder never passes `max_age`, so expiry comes from
    /// the `sessions.expire_date` column (see [`crate::session`]), not from
    /// the embedded timestamp. The timestamp is still parsed — a corrupt one
    /// is a malformed value, not a valid one.
    pub fn unsign(&self, signed: &str) -> Result<String, SigningError> {
        let (value, signature) = signed.rsplit_once(':').ok_or(SigningError::BadSignature)?;
        self.verify(value, signature)?;
        let (value, timestamp) = value.rsplit_once(':').ok_or(SigningError::Malformed)?;
        decode_base62(timestamp).ok_or(SigningError::Malformed)?;
        Ok(value.to_owned())
    }

    /// `TimestampSigner.sign_object` with `compress=True`: JSON-encode with
    /// compact separators, zlib-compress when that shortens the payload by
    /// more than one byte (the `.` prefix marks compression and is part of
    /// the signed value), then timestamp and sign with the primary key.
    ///
    /// Django compresses with `zlib.compress` at default level; this uses
    /// flate2's default level too. Compressed bytes may differ between the
    /// two zlib implementations — only decompression equivalence is
    /// guaranteed, which is all any reader needs.
    pub fn sign_object<T: serde::Serialize>(
        &self,
        obj: &T,
        unix_time_secs: u64,
    ) -> Result<String, SigningError> {
        let data = serde_json::to_vec(obj).map_err(|_| SigningError::Json)?;
        let compressed = {
            use flate2::read::ZlibEncoder;
            use flate2::Compression;
            use std::io::Read;
            let mut out = Vec::new();
            ZlibEncoder::new(&data[..], Compression::default())
                .read_to_end(&mut out)
                .map_err(|_| SigningError::Json)?;
            out
        };
        let (mut encoded, compressed) = if compressed.len() + 1 < data.len() {
            (compressed, true)
        } else {
            (data, false)
        };
        let mut base64d = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&encoded);
        if compressed {
            encoded = format!(".{base64d}").into_bytes();
            base64d = String::from_utf8(encoded).expect("base64url is ASCII");
        }
        let key = self.keys.first().expect("signer always holds a key");
        let value = format!("{base64d}:{}", encode_base62(unix_time_secs));
        Ok(format!("{value}:{}", self.signature(&value, key)))
    }

    /// `TimestampSigner.unsign_object`: verify, base64-decode, decompress a
    /// leading-`.` payload, and parse the JSON body.
    pub fn unsign_object<T: serde::de::DeserializeOwned>(
        &self,
        signed: &str,
    ) -> Result<T, SigningError> {
        let mut encoded = self.unsign(signed)?.into_bytes();
        let compressed = encoded.first() == Some(&b'.');
        if compressed {
            encoded.remove(0);
        }
        let data = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&encoded)
            .map_err(|_| SigningError::Malformed)?;
        let data = if compressed {
            use flate2::read::ZlibDecoder;
            use std::io::Read;
            let mut out = Vec::new();
            ZlibDecoder::new(&data[..])
                .read_to_end(&mut out)
                .map_err(|_| SigningError::Decompress)?;
            out
        } else {
            data
        };
        serde_json::from_slice(&data).map_err(|_| SigningError::Json)
    }
}

/// Encode a timestamp the way `TimestampSigner.timestamp` does.
fn encode_base62(mut value: u64) -> String {
    if value == 0 {
        return "0".to_owned();
    }
    let mut out = Vec::new();
    while value > 0 {
        out.push(BASE62_ALPHABET[(value % 62) as usize]);
        value /= 62;
    }
    out.iter().rev().map(|b| *b as char).collect()
}

/// Decode Django's base62 timestamp. Returns `None` on empty input or on a
/// character outside `0-9A-Za-z` (including the `-` sign Django only emits
/// for negative numbers, which never occur as timestamps).
fn decode_base62(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut value: u64 = 0;
    for byte in s.bytes() {
        let digit = BASE62_ALPHABET.iter().position(|b| *b == byte)? as u64;
        value = value.checked_mul(62)?.checked_add(digit)?;
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"f05-test-secret-key-12345";

    fn session_signer() -> Signer {
        Signer::new(SECRET, SESSION_SIGNING_SALT)
    }

    // Value produced by Django 6.0.5:
    // signing.dumps({...big session...}, salt="django.contrib.sessions.SessionStore",
    //                 serializer=JSONSerializer, compress=True)
    const DJANGO_COMPRESSED: &str = ".eJxVjEEOwiAQRe_Cup3YFkvoTj2DazKF0RItkEKNifHuQuNCl_PnvfdiCtc0qTXSoqxhAxNaUt8LWQve7mu-M1RLzseaWjnuSXRC7jpW_Woj6hu54garDMYJyo9cshqT9Q7QYEi0gF7IlBnvcFpj8vMhc8ev_ZecciX3cNRN221HxQw9rCZl3cWz4cU2Dq-5l8HzIQM2KDRmoRjz0kALHfDi-RmtK7UQgJ44hzuB9jN7vz8DyFT-:1xAYRV:-9lka6d1CruUv4KZTvhYIUY9g4AO6iV4PW0-zh110xc";
    // signing.dumps({"_auth_user_id": "1"}, salt=..., compress=True)
    // (too short to benefit from compression, so stored uncompressed).
    const DJANGO_SMALL: &str =
        "eyJfYXV0aF91c2VyX2lkIjoiMSJ9:1xAYRV:fxj6GrpyAaLedQgLHyVlsGBoexvGOUKH5qqVEQzR2MY";

    #[test]
    fn decodes_django_compressed_session() {
        let data: serde_json::Value = session_signer().unsign_object(DJANGO_COMPRESSED).unwrap();
        assert_eq!(
            data["_auth_user_id"],
            "7c9e6679-7425-40de-944b-e29b5e737903"
        );
        assert_eq!(
            data["_auth_user_backend"],
            "pi_dash.authentication.adapter.credential.CustomAuthBackend"
        );
        assert_eq!(data["device_info"]["ip_address"], "1.2.3.4");
    }

    #[test]
    fn decodes_django_uncompressed_session() {
        let data: serde_json::Value = session_signer().unsign_object(DJANGO_SMALL).unwrap();
        assert_eq!(data["_auth_user_id"], "1");
    }

    #[test]
    fn tampered_payload_fails() {
        let mut bad = DJANGO_SMALL.to_owned();
        bad.replace_range(4..5, "A");
        assert_eq!(
            session_signer()
                .unsign_object::<serde_json::Value>(&bad)
                .unwrap_err(),
            SigningError::BadSignature
        );
    }

    #[test]
    fn wrong_secret_fails() {
        let signer = Signer::new(b"another-secret", SESSION_SIGNING_SALT);
        assert_eq!(
            signer
                .unsign_object::<serde_json::Value>(DJANGO_SMALL)
                .unwrap_err(),
            SigningError::BadSignature
        );
    }

    #[test]
    fn fallback_key_verifies() {
        let signer =
            Signer::with_fallbacks(b"fresh-secret", &[SECRET.to_vec()], SESSION_SIGNING_SALT);
        let data: serde_json::Value = signer.unsign_object(DJANGO_SMALL).unwrap();
        assert_eq!(data["_auth_user_id"], "1");
    }

    #[test]
    fn missing_separator_is_bad_signature() {
        assert_eq!(
            session_signer().unsign("nosignature").unwrap_err(),
            SigningError::BadSignature
        );
    }

    #[test]
    fn corrupt_timestamp_is_malformed() {
        // Re-sign a value whose timestamp contains a character outside base62.
        let signer = session_signer();
        let forged_value = "eA:!!!";
        let sig = signer.signature(forged_value, SECRET);
        let err = signer
            .unsign_object::<serde_json::Value>(&format!("{forged_value}:{sig}"))
            .unwrap_err();
        assert_eq!(err, SigningError::Malformed);
    }

    #[test]
    fn base62_vectors() {
        assert_eq!(decode_base62("0"), Some(0));
        assert_eq!(decode_base62("1xAYRV"), Some(1790452337));
        assert_eq!(decode_base62(""), None);
        assert_eq!(decode_base62("!"), None);
        assert_eq!(decode_base62("-1"), None);
    }

    #[test]
    fn base62_round_trip() {
        for ts in [0u64, 1, 61, 62, 1790452337, 2000000000, u64::MAX] {
            assert_eq!(decode_base62(&encode_base62(ts)), Some(ts));
        }
    }

    #[test]
    fn rust_signed_value_verifies_and_decodes() {
        let signer = session_signer();
        let obj = serde_json::json!({"_auth_user_id": "42", "k": "v".repeat(50)});
        let signed = signer.sign_object(&obj, 1790452337).unwrap();
        let back: serde_json::Value = signer.unsign_object(&signed).unwrap();
        assert_eq!(back, obj);
    }
}
