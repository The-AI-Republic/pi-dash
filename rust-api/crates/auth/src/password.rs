#![forbid(unsafe_code)]

//! Django password-hash verification (`PBKDF2PasswordHasher`).
//!
//! Python reference: `django.contrib.auth.hashers.PBKDF2PasswordHasher`
//! (Django 6.0). The stored form is
//!
//! ```text
//! pbkdf2_sha256$<iterations>$<salt>$<base64(PBKDF2-HMAC-SHA256, 32 bytes)>
//! ```
//!
//! Verification re-encodes the candidate with the salt and iteration count
//! taken from the stored string and compares the two full encodings in
//! constant time — Django compares the whole `algorithm$iterations$salt$hash`
//! string, not just the digest, so a parameters mismatch fails closed here
//! too. Only `pbkdf2_sha256` is accepted: that is the project's sole
//! configured hasher, and anything else is an error rather than a guess.

use base64::Engine;
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;
use subtle::ConstantTimeEq;
use thiserror::Error as ThisError;

/// Algorithm tag Django writes into the hash string.
pub const PBKDF2_ALGORITHM: &str = "pbkdf2_sha256";
/// Iteration count Django 6.0 uses for newly set passwords.
pub const PBKDF2_DEFAULT_ITERATIONS: u32 = 1_200_000;
/// PBKDF2 output length in bytes (Django documents a 64-byte string, but the
/// hasher requests 32 bytes from the PRF — the stored base64 decodes to 32).
pub const PBKDF2_DKLEN: usize = 32;

/// Why a stored hash could not be verified against.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum PasswordError {
    #[error("password hash is malformed")]
    Malformed,
    #[error("unsupported password hash algorithm: {0}")]
    UnsupportedAlgorithm(String),
}

/// Hash a password exactly like Django's `PBKDF2PasswordHasher.encode`.
///
/// `salt` must not contain `$` (Django raises there too); the salt Django
/// generates is 22 alphanumeric chars, but verification accepts whatever
/// the stored string carries.
pub fn hash_password(password: &str, salt: &str, iterations: u32) -> String {
    assert!(!salt.contains('$'), "salt must not contain '$'");
    let mut out = [0u8; PBKDF2_DKLEN];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt.as_bytes(), iterations, &mut out);
    format!(
        "{PBKDF2_ALGORITHM}${iterations}${salt}${}",
        base64::engine::general_purpose::STANDARD.encode(out)
    )
}

/// Verify a candidate password against a stored Django hash.
///
/// Returns `Ok(true)` on match, `Ok(false)` on mismatch, and `Err` when the
/// stored string is not a `pbkdf2_sha256` hash at all.
pub fn verify_password(password: &str, encoded: &str) -> Result<bool, PasswordError> {
    let mut parts = encoded.split('$');
    let (algorithm, iterations, salt) = match (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) {
        (Some(algorithm), Some(iterations), Some(salt), Some(_), None) => {
            (algorithm, iterations, salt)
        }
        _ => return Err(PasswordError::Malformed),
    };
    if algorithm != PBKDF2_ALGORITHM {
        return Err(PasswordError::UnsupportedAlgorithm(algorithm.to_owned()));
    }
    let iterations: u32 = iterations.parse().map_err(|_| PasswordError::Malformed)?;
    if iterations == 0 {
        return Err(PasswordError::Malformed);
    }
    let candidate = hash_password(password, salt, iterations);
    Ok(bool::from(candidate.as_bytes().ct_eq(encoded.as_bytes())))
}

/// True when the stored hash would be re-hashed on next login.
///
/// Mirrors `must_update` ignoring the salt-entropy leg (Django also
/// re-hashes short salts; ours are always Django-generated): any iteration
/// count other than the current default wants an upgrade.
pub fn needs_update(encoded: &str) -> Result<bool, PasswordError> {
    let mut parts = encoded.split('$');
    match (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) {
        (Some(algorithm), Some(iterations), Some(_), Some(_), None) => {
            if algorithm != PBKDF2_ALGORITHM {
                return Err(PasswordError::UnsupportedAlgorithm(algorithm.to_owned()));
            }
            let iterations: u32 = iterations.parse().map_err(|_| PasswordError::Malformed)?;
            Ok(iterations != PBKDF2_DEFAULT_ITERATIONS)
        }
        _ => Err(PasswordError::Malformed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hashes produced by Django 6.0.5's PBKDF2PasswordHasher.encode.
    const DJANGO_LOW: &str =
        "pbkdf2_sha256$26000$f05saltABCDEF1234567890ab$5Yruc/dEr8aU75Iv23Hue6WzOB7d/jOY4VVwoakOiHQ=";
    const DJANGO_DEFAULT: &str =
        "pbkdf2_sha256$1200000$defaultsalt0123456789abcdef$L8PcI/YkM3HgrvkCRoWMuCH1vp8u0LY9lGKMiVyPYWE=";

    #[test]
    fn verifies_django_hash() {
        assert!(verify_password("correct-horse", DJANGO_LOW).unwrap());
        assert!(!verify_password("wrong-horse", DJANGO_LOW).unwrap());
    }

    #[test]
    fn verifies_django_default_iterations() {
        assert!(verify_password("s3cr3t-pw", DJANGO_DEFAULT).unwrap());
        assert!(!verify_password("s3cr3t-pw!", DJANGO_DEFAULT).unwrap());
    }

    #[test]
    fn hash_matches_django_byte_for_byte() {
        assert_eq!(
            hash_password("correct-horse", "f05saltABCDEF1234567890ab", 26000),
            DJANGO_LOW
        );
    }

    #[test]
    fn tampered_digest_fails() {
        let mut bad = DJANGO_LOW.to_owned();
        bad.replace_range(60..61, "A");
        assert!(!verify_password("correct-horse", &bad).unwrap());
    }

    #[test]
    fn tampered_iterations_fail_closed() {
        // Changing the iteration count changes the re-encoded candidate, so
        // the full-string comparison fails instead of verifying under the
        // attacker's parameters.
        let bad = DJANGO_LOW.replacen("$26000$", "$2600$", 1);
        assert!(!verify_password("correct-horse", &bad).unwrap());
    }

    #[test]
    fn rejects_unknown_algorithm() {
        assert_eq!(
            verify_password("x", "bcrypt$12$salt$hash").unwrap_err(),
            PasswordError::UnsupportedAlgorithm("bcrypt".to_owned())
        );
    }

    #[test]
    fn rejects_malformed() {
        for bad in [
            "",
            "pbkdf2_sha256",
            "pbkdf2_sha256$abc$s$h",
            "a$b$c$d$e",
            "pbkdf2_sha256$0$s$h",
        ] {
            assert_eq!(
                verify_password("x", bad).unwrap_err(),
                PasswordError::Malformed,
                "{bad}"
            );
        }
    }

    #[test]
    fn update_check() {
        assert!(!needs_update(DJANGO_DEFAULT).unwrap());
        assert!(needs_update(DJANGO_LOW).unwrap());
    }
}
