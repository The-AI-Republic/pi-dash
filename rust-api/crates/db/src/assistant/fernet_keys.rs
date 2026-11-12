//! Multi-key Fernet primitive for the assistant crypto backend.
//!
//! Ports the raw cipher half of `apps/api/pi_dash/assistant/crypto.py:134-190`
//! (`FernetBackend` over `cryptography.fernet.MultiFernet`). It lives in
//! `pidash-db` because that is where the locked `fernet 0.2` dependency
//! already is; all backend/config/error-mapping logic lives in
//! `pidash-services::assistant::crypto`, which delegates here.
//!
//! Byte compatibility: the `fernet` crate implements the same token format
//! as Python's `cryptography` package, so ciphertext stored by Django
//! decrypts here and vice versa (pinned by
//! [`tests::decrypts_python_generated_token`]). Tokens cross as bytes —
//! Python's `encrypt` returns the base64 token as `bytes` and the
//! `api_key_encrypted` column is a `BinaryField` — so `encrypt` returns the
//! UTF-8 token bytes and `decrypt`/`rotate` take them back.
//!
//! Failure shape: every failure here is "ciphertext I can't decrypt with
//! this key" or "no usable key", which the services layer reports as
//! `AssistantNotConfigured` (`crypto.py:19-23`). Operational failures cannot
//! arise below this API (no I/O), so a plain error enum suffices.

/// Key list or token this keyring cannot use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FernetKeyError {
    /// No key configured (empty/blank key list).
    NoKeys,
    /// At least one key is not a valid Fernet key.
    InvalidKey,
    /// The token does not decrypt under any configured key.
    InvalidToken,
}

impl std::fmt::Display for FernetKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FernetKeyError::NoKeys => write!(f, "no Fernet keys configured"),
            FernetKeyError::InvalidKey => write!(f, "invalid Fernet key"),
            FernetKeyError::InvalidToken => write!(f, "Fernet token did not decrypt"),
        }
    }
}

impl std::error::Error for FernetKeyError {}

/// Multi-key Fernet keyring (`crypto.py:137-140`): the first key encrypts
/// new values, all keys decrypt existing ones, so operators prepend a new
/// key and call `rotate` without downtime. `Debug` is redacted: key
/// material never reaches logs.
pub struct FernetKeyring {
    inner: fernet::MultiFernet,
}

impl std::fmt::Debug for FernetKeyring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FernetKeyring").finish_non_exhaustive()
    }
}

impl FernetKeyring {
    /// Parse a comma-separated key list (`crypto.py:145-147`): entries are
    /// stripped, blanks dropped. Empty input yields [`FernetKeyError::NoKeys`]
    /// and an undecodable key yields [`FernetKeyError::InvalidKey`], mirroring
    /// the `AssistantNotConfigured` branches of `_require_fernet`
    /// (`crypto.py:153-159`).
    ///
    /// Note the `fernet` crate's `MultiFernet::new` panics on an empty vec;
    /// the `NoKeys` guard below keeps that panic unreachable.
    pub fn from_comma_list(raw: &str) -> Result<Self, FernetKeyError> {
        let keys: Vec<fernet::Fernet> = raw
            .split(',')
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(|key| fernet::Fernet::new(key).ok_or(FernetKeyError::InvalidKey))
            .collect::<Result<Vec<_>, _>>()?;
        if keys.is_empty() {
            return Err(FernetKeyError::NoKeys);
        }
        Ok(Self {
            inner: fernet::MultiFernet::new(keys),
        })
    }

    /// Encrypt with the first key (`crypto.py:171-172`); returns the base64
    /// token as bytes, exactly what Python stores in `api_key_encrypted`.
    pub fn encrypt(&self, plaintext: &[u8]) -> Vec<u8> {
        self.inner.encrypt(plaintext).into_bytes()
    }

    /// Decrypt under any key (`crypto.py:174-182`); undecryptable (or
    /// non-UTF-8) tokens yield [`FernetKeyError::InvalidToken`], mirroring
    /// the `InvalidToken -> AssistantNotConfigured` branch.
    pub fn decrypt(&self, token: &[u8]) -> Result<Vec<u8>, FernetKeyError> {
        let token = std::str::from_utf8(token).map_err(|_| FernetKeyError::InvalidToken)?;
        self.inner
            .decrypt(token)
            .map_err(|_| FernetKeyError::InvalidToken)
    }

    /// Re-encrypt under the first key (`crypto.py:184-190`); undecryptable
    /// tokens yield [`FernetKeyError::InvalidToken`] like Python's `rotate`.
    ///
    /// Timestamp note: Python's `MultiFernet.rotate` preserves the token's
    /// original timestamp while this decrypts and re-encrypts (fresh
    /// timestamp), because the `fernet` crate exposes no timestamp-preserving
    /// rotate. The difference is unobservable here — nothing reads token
    /// timestamps (no TTL checks anywhere on this path) — and the output
    /// decrypts identically.
    pub fn rotate(&self, token: &[u8]) -> Result<Vec<u8>, FernetKeyError> {
        let plaintext = self.decrypt(token)?;
        Ok(self.encrypt(&plaintext))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Python-generated cross-compatibility vector (`cryptography` 3.4.8):
    // K1 encrypts b"sk-live-key-123"; the K2,K1 ring must still
    // decrypt it, and rotating onto K2 must round-trip.
    const K1: &str = "Lk0_5ciI2pVpujWpFeeysfbbGfRDTBMu-tNTJb7lLMw=";
    const K2: &str = "INrRUbePFWt0XS3zTLayhRBYicavxkDznAAxc8BBbr0=";
    const PYTHON_TOKEN: &str = "gAAAAABqu3DT7NWuBYgWhqKBzL4kSWPefHCTJIUedSSU8p2e_Kv8ey-DgaYGJgXY7cl_Pzb-NzdH2k30IhIapHseLGJHilF13w==";

    #[test]
    fn round_trip_bytes() {
        let ring = FernetKeyring::from_comma_list(K1).expect("valid key");
        let token = ring.encrypt(b"sk-live");
        assert_eq!(ring.decrypt(&token).expect("decrypts"), b"sk-live");
    }

    #[test]
    fn decrypts_python_generated_token() {
        let ring = FernetKeyring::from_comma_list(&format!("{K2},{K1}")).expect("valid keys");
        assert_eq!(
            ring.decrypt(PYTHON_TOKEN.as_bytes())
                .expect("python token decrypts"),
            b"sk-live-key-123"
        );
    }

    #[test]
    fn prepended_key_still_decrypts_old_token() {
        let ring = FernetKeyring::from_comma_list(&format!("{K2},{K1}")).expect("valid keys");
        let rotated = ring.rotate(PYTHON_TOKEN.as_bytes()).expect("rotates");
        // Rotated onto the first key (K2): the old ring still reads it, and a
        // K2-only ring does too, proving re-encryption under the first key.
        assert_eq!(
            ring.decrypt(&rotated).expect("decrypts"),
            b"sk-live-key-123"
        );
        let k2_only = FernetKeyring::from_comma_list(K2).expect("valid key");
        assert_eq!(
            k2_only.decrypt(&rotated).expect("decrypts"),
            b"sk-live-key-123"
        );
    }

    #[test]
    fn wrong_key_fails() {
        let ring = FernetKeyring::from_comma_list(K2).expect("valid key");
        assert_eq!(
            ring.decrypt(PYTHON_TOKEN.as_bytes()),
            Err(FernetKeyError::InvalidToken)
        );
        assert_eq!(
            ring.rotate(PYTHON_TOKEN.as_bytes()),
            Err(FernetKeyError::InvalidToken)
        );
    }

    #[test]
    fn empty_list_has_no_keys() {
        for raw in ["", "   ", " , , "] {
            assert_eq!(
                FernetKeyring::from_comma_list(raw).unwrap_err(),
                FernetKeyError::NoKeys
            );
        }
    }

    #[test]
    fn invalid_key_rejected() {
        assert_eq!(
            FernetKeyring::from_comma_list("not-a-key").unwrap_err(),
            FernetKeyError::InvalidKey
        );
    }

    #[test]
    fn non_utf8_token_fails() {
        let ring = FernetKeyring::from_comma_list(K1).expect("valid key");
        assert_eq!(
            ring.decrypt(&[0xff, 0xfe]),
            Err(FernetKeyError::InvalidToken)
        );
    }

    #[test]
    fn rotate_empty_token_fails_like_python() {
        // `rotate` has no empty short-circuit (`crypto.py:184-190`):
        // `MultiFernet.rotate(b"")` raises `InvalidToken`.
        let ring = FernetKeyring::from_comma_list(K1).expect("valid key");
        assert_eq!(ring.rotate(b""), Err(FernetKeyError::InvalidToken));
    }

    #[test]
    fn debug_redacts_keys() {
        let ring = FernetKeyring::from_comma_list(K1).expect("valid key");
        let rendered = format!("{ring:?}");
        assert!(!rendered.contains(K1), "key material must not reach Debug");
    }
}
