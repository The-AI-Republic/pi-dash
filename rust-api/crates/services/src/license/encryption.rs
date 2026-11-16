//! License-domain encryption (D-01 utils).
//!
//! Port of `apps/api/pi_dash/license/utils/encryption.py:13-44`:
//!
//! * `derive_key(secret_key)` — PBKDF2-HMAC-SHA256, 100 000 rounds, salt
//!   `b"salt"`, 32 bytes, URL-safe base64 (`encryption.py:13-16`).
//! * `encrypt_data(data)` — Fernet under the key derived from
//!   `settings.SECRET_KEY`; falsy input yields `""` without touching Fernet
//!   (`:22,26-27`); any exception is logged and yields `""` (`:28-30`).
//! * `decrypt_data(token)` — Fernet decrypt, UTF-8 decode; falsy input yields
//!   `""` (`:34-41`); any exception (`InvalidToken`, bad UTF-8, …) is logged
//!   and yields `""` (`:42-44`).
//!
//! Implemented by the F-03 kernel (`pidash_db::config::encryption`); this
//! module keeps the domain's Python-shaped calling convention (free functions
//! named after the Python units, `Option<&str>` for the nullable path).
//!
//! Ported bugs / inherited semantics (translate, don't redesign):
//!
//! * Exception branches degrade to `""` instead of an error, so a rotated or
//!   corrupt secret reads as an empty value (kernel `Keyring`, same wart).
//! * The kernel decodes decrypted bytes with UTF-8 lossy conversion while
//!   Python's strict `.decode()` would raise (and yield `""`). Only reachable
//!   with a token whose payload is non-UTF-8 under the right key, which
//!   requires key knowledge; behavior is otherwise identical.
//! * `log_exception` (`pi_dash/utils/exception_logger.py`) becomes the
//!   kernel's `tracing::warn!` (same "log and return empty" contract).

pub use pidash_db::config::encryption::{derive_fernet_key, Keyring};

/// Port of `derive_key` (`encryption.py:13-16`).
///
/// `urlsafe_b64encode(pbkdf2_hmac("sha256", secret, b"salt", 100_000))`.
/// Derives for any input, including `""` — the falsy gate lives on the data,
/// not the key (fixture `encryption.golden.json`, `derive_key_vectors`).
pub fn derive_key(secret_key: &str) -> String {
    derive_fernet_key(secret_key)
}

/// Port of `encrypt_data` (`encryption.py:20-30`).
///
/// `None` and `""` yield `""` without touching Fernet (`:22,26-27`); anything
/// else is Fernet-encrypted under `keyring` (`:23-25`, key derived from
/// `settings.SECRET_KEY` — build the keyring with [`Keyring::from_env`],
/// which reads the same `SECRET_KEY` process variable).
pub fn encrypt_data(keyring: &Keyring, data: Option<&str>) -> String {
    match data {
        Some(text) if !text.is_empty() => keyring.encrypt(text),
        _ => String::new(),
    }
}

/// Port of `decrypt_data` (`encryption.py:34-44`).
///
/// `None` and `""` yield `""` (`:36-41`); anything else is Fernet-decrypted
/// (`:37-39`); any failure is logged and yields `""` (`:42-44`, kernel
/// `tracing::warn!` standing in for `log_exception`).
pub fn decrypt_data(keyring: &Keyring, encrypted_data: Option<&str>) -> String {
    match encrypted_data {
        Some(token) if !token.is_empty() => keyring.decrypt(token),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-key";

    fn keyring() -> Keyring {
        Keyring::from_secret(TEST_SECRET)
    }

    #[test]
    fn derive_key_matches_golden_vectors() {
        // Fixture utils/encryption.golden.json, derive_key_vectors — both
        // vectors executed live against the Python source at fixture time.
        assert_eq!(
            derive_key("test-secret-key"),
            "g7t9c4Ah6MVui10RpWHy8k1gT-_hCE8qhrHSqX0so2k="
        );
        assert_eq!(
            derive_key(""),
            "N4qL2pdq3gpEl9KgS9HONSvGbDiZR2JK5fq4xiB0o3E="
        );
    }

    #[test]
    fn encrypt_falsy_yields_empty_without_touching_fernet() {
        // Fixture encrypt_data_cases: "" and null -> "" (encryption.py:22,26-27).
        let k = keyring();
        assert_eq!(encrypt_data(&k, None), "");
        assert_eq!(encrypt_data(&k, Some("")), "");
    }

    #[test]
    fn encrypt_format_matches_fernet_properties() {
        // Fixture encrypt_data_cases: nondeterministic output (random IV), so
        // the vector asserts stable properties instead: "gAAAAA" prefix and
        // 100 chars for a 12-byte plaintext.
        let token = encrypt_data(&keyring(), Some("s3cr3t-value"));
        assert!(token.starts_with("gAAAAA"), "unexpected token {token}");
        assert_eq!(token.len(), 100, "unexpected token {token}");
    }

    #[test]
    fn roundtrip_through_rust() {
        // Fixture "roundtrip": decrypt_data(encrypt_data(s)) == s.
        let k = keyring();
        let token = encrypt_data(&k, Some("s3cr3t-value"));
        assert_eq!(decrypt_data(&k, Some(&token)), "s3cr3t-value");
    }

    #[test]
    fn decrypts_python_generated_token() {
        // Token produced by the real Python implementation
        // (hashlib.pbkdf2_hmac + cryptography.Fernet, secret
        // "test-secret-key", plaintext "s3cr3t-value"). Plain Fernet decrypt
        // performs no TTL check, so the vector is stable.
        let token = "gAAAAABquI1fvxoiWw1JS1ueZpZDoUe9UBSxC7AKq6sUe9fh6T9LP0GtqXYq7U5zLJ3fFfgXRC5NUqIDtM27eHiELz2AiSnRbw==";
        assert_eq!(decrypt_data(&keyring(), Some(token)), "s3cr3t-value");
    }

    #[test]
    fn decrypt_falsy_yields_empty() {
        // Fixture decrypt_data_cases: "" and null -> "".
        let k = keyring();
        assert_eq!(decrypt_data(&k, Some("")), "");
        assert_eq!(decrypt_data(&k, None), "");
    }

    #[test]
    fn decrypt_exception_branches_yield_empty() {
        // Fixture decrypt_data_cases: InvalidToken (encryption.py:42-44)
        // degrades to "" after logging, for both a malformed token and a
        // well-formed token under the wrong key.
        let k = keyring();
        assert_eq!(decrypt_data(&k, Some("not-a-token")), "");
        let foreign = Keyring::from_secret("other-secret").encrypt("s3cr3t-value");
        assert_eq!(decrypt_data(&k, Some(&foreign)), "");
    }
}
