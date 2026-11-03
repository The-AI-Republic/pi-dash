#![forbid(unsafe_code)]

//! Django password-hash verification (`PBKDF2PasswordHasher`).
//!
//! Python reference: `django.contrib.auth.hashers.PBKDF2PasswordHasher`
//! (Django 4.2, the pinned version). The stored form is
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
/// Iteration count the pinned Django 4.2 uses for newly set passwords
/// (`PBKDF2PasswordHasher.iterations = 600000`; Django 6.0 raised it to
/// 1_200_000, but this project pins Django 4.2.30).
pub const PBKDF2_DEFAULT_ITERATIONS: u32 = 600_000;
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

/// Salt length (chars) at which Django stops re-hashing for entropy.
///
/// `must_update_salt` (`hashers.py:183-185`) wants 128 bits over the
/// 62-char `RANDOM_STRING_CHARS` alphabet: 21 chars carry
/// 21 * log2(62) = 125.0 bits (< 128, update) while 22 carry 131.0
/// (>= 128, keep). Django-generated salts are always 22 chars; shorter
/// ones come from older installs.
pub const PBKDF2_FULL_ENTROPY_SALT_LEN: usize = 22;

/// True when the stored hash would be re-hashed on next login.
///
/// Mirrors `PBKDF2PasswordHasher.must_update` (`hashers.py:339-342`):
/// any iteration count other than the current default wants an
/// upgrade, as does a salt below [`PBKDF2_FULL_ENTROPY_SALT_LEN`].
pub fn needs_update(encoded: &str) -> Result<bool, PasswordError> {
    let mut parts = encoded.split('$');
    match (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) {
        (Some(algorithm), Some(iterations), Some(salt), Some(_), None) => {
            if algorithm != PBKDF2_ALGORITHM {
                return Err(PasswordError::UnsupportedAlgorithm(algorithm.to_owned()));
            }
            let iterations: u32 = iterations.parse().map_err(|_| PasswordError::Malformed)?;
            Ok(
                iterations != PBKDF2_DEFAULT_ITERATIONS
                    || salt.len() < PBKDF2_FULL_ENTROPY_SALT_LEN,
            )
        }
        _ => Err(PasswordError::Malformed),
    }
}

/// Replacement hash when Django's `check_password` setter would fire.
///
/// `hashers.check_password` (`hashers.py:38-73`) calls the setter — which
/// re-encodes with the default hasher and saves — exactly when the
/// candidate verifies and [`needs_update`]. Returns that re-encoding
/// (`make_password(candidate)`: fresh 22-char salt, default iterations),
/// or `None` when the password is wrong or the stored hash is current,
/// so callers can `UPDATE users SET password` only on `Some`.
pub fn rehash_if_outdated(candidate: &str, encoded: &str) -> Option<String> {
    if !verify_password(candidate, encoded).unwrap_or(false) {
        return None;
    }
    if !needs_update(encoded).unwrap_or(false) {
        return None;
    }
    use rand::distr::{Alphanumeric, SampleString};
    let salt = Alphanumeric.sample_string(&mut rand::rng(), PBKDF2_FULL_ENTROPY_SALT_LEN);
    Some(hash_password(candidate, &salt, PBKDF2_DEFAULT_ITERATIONS))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hashes produced by Django 4.2.30's PBKDF2PasswordHasher.encode.
    const DJANGO_LOW: &str =
        "pbkdf2_sha256$26000$f05saltABCDEF1234567890ab$5Yruc/dEr8aU75Iv23Hue6WzOB7d/jOY4VVwoakOiHQ=";
    const DJANGO_DEFAULT: &str =
        "pbkdf2_sha256$600000$defaultsalt0123456789abcdef$8Fue9iT8ik6C0dEhPy8cJlFJMLuayJi8LMN3ceyFSn0=";

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
    fn default_iterations_match_django() {
        // Guards the F-05 bug (PIDASHCONV-855): the default must be the
        // pinned Django 4.2's 600000, not Django 6.0's 1200000.
        assert_eq!(
            hash_password(
                "s3cr3t-pw",
                "defaultsalt0123456789abcdef",
                PBKDF2_DEFAULT_ITERATIONS
            ),
            DJANGO_DEFAULT
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

    #[test]
    fn update_check_salt_entropy_leg() {
        // Default iterations but a 21-char salt: 125.0 bits < 128, Django
        // re-hashes (`must_update_salt`). A 22-char salt is current.
        let short = hash_password("s3cr3t-pw", "shortsalt0123456789AB", 600_000);
        assert_eq!(short.split('$').nth(2).unwrap().len(), 21);
        assert!(needs_update(&short).unwrap());
        let full = hash_password("s3cr3t-pw", "fullsalt0123456789ABCD", 600_000);
        assert_eq!(full.split('$').nth(2).unwrap().len(), 22);
        assert!(!needs_update(&full).unwrap());
    }

    #[test]
    fn rehash_returns_new_default_hash_for_outdated() {
        let new = rehash_if_outdated("correct-horse", DJANGO_LOW).expect("outdated re-hashes");
        assert_ne!(new, DJANGO_LOW);
        assert!(new.starts_with("pbkdf2_sha256$600000$"), "{new}");
        assert_eq!(new.split('$').nth(2).unwrap().len(), 22);
        assert!(verify_password("correct-horse", &new).unwrap());
    }

    #[test]
    fn rehash_returns_none_for_current_or_wrong() {
        assert_eq!(rehash_if_outdated("s3cr3t-pw", DJANGO_DEFAULT), None);
        assert_eq!(rehash_if_outdated("wrong-horse", DJANGO_LOW), None);
        assert_eq!(rehash_if_outdated("x", "not-a-hash"), None);
    }
}
