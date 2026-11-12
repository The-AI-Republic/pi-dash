//! At-rest encryption for BYOK LLM API keys (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/crypto.py:1-233`: the `CipherBackend`
//! seam, the AWS KMS and local Fernet backends, the backend registry, and the
//! `is_configured` / `encrypt` / `decrypt` / `rotate` public API. Fixture id
//! F-A6-04 (`rust-api/fixtures/assistant/crypto.json`).
//!
//! Django `settings` reads cross as [`CryptoConfig`] (the `GitLabConfig`
//! precedent: settings never reach library code). The process-wide cached
//! backend (`crypto.py:201,204-214`) likewise becomes an explicit value:
//! [`Backend::from_config`].
//!
//! The KMS backend runs against an injected [`KmsTransport`] — this crate
//! holds no AWS client (the `GitLabTransport` precedent: network I/O lives
//! with the later wiring issue that serves these methods, unit tests inject
//! a recording fake). The trait carries the exact `boto3` contract Python
//! relies on: `decrypt` pins `KeyId`, `re_encrypt` passes only
//! `CiphertextBlob` + `DestinationKeyId`, and errors model
//! `ClientError.response.Error.Code` (`code`) versus transport-level
//! `BotoCoreError` (empty `code`).
//!
//! Failure conventions (`crypto.py:19-23`): undecryptable/not-configured
//! surfaces as [`CryptoError::NotConfigured`] wrapping
//! `AssistantNotConfigured`; operational failures propagate as
//! [`CryptoError::Transport`].
//!
//! Ported bugs (translate, don't redesign — each asserted below):
//!
//! * KMS `encrypt` wraps even operational failures in `NotConfigured`
//!   (`crypto.py:103-106`); only `decrypt` distinguishes undecryptable
//!   codes from operational ones.
//! * The default backend is `aws-kms` when `ASSISTANT_CRYPTO_BACKEND` is
//!   unset or empty (`crypto.py:207`).

use pidash_db::assistant::fernet_keys::{FernetKeyError, FernetKeyring};
use pidash_types::assistant::AssistantError;

/// Registry name of the AWS KMS backend (`crypto.py:196`).
pub const BACKEND_AWS_KMS: &str = "aws-kms";
/// Registry name of the local Fernet backend (`crypto.py:197`).
pub const BACKEND_FERNET: &str = "fernet";
/// Backend used when `ASSISTANT_CRYPTO_BACKEND` is unset or empty
/// (`crypto.py:207`).
pub const DEFAULT_BACKEND: &str = BACKEND_AWS_KMS;
/// Registry names in Python's `sorted(_BACKENDS)` order, for the
/// unknown-backend message (`crypto.py:211`).
pub const KNOWN_BACKENDS: &[&str] = &[BACKEND_AWS_KMS, BACKEND_FERNET];
/// KMS error codes meaning "this ciphertext can't be decrypted with this
/// key" (`crypto.py:71`); everything else operational propagates.
pub const KMS_UNDECRYPTABLE_CODES: &[&str] = &[
    "InvalidCiphertextException",
    "IncorrectKeyException",
    "NotFoundException",
];

/// Backend settings (`crypto.py:62-64,137-140,207`).
#[derive(Debug, Clone, Default)]
pub struct CryptoConfig {
    /// `ASSISTANT_CRYPTO_BACKEND` (`aws-kms` / `fernet`).
    pub backend: String,
    /// `ASSISTANT_ENCRYPTION_KEY`, comma-separated (first encrypts).
    pub fernet_keys: String,
    /// `ASSISTANT_KMS_KEY_ID` (CMK id/ARN/alias).
    pub kms_key_id: String,
    /// `AWS_REGION`.
    pub aws_region: String,
    /// `ASSISTANT_KMS_ENDPOINT_URL` (e.g. LocalStack).
    pub kms_endpoint_url: String,
}

impl CryptoConfig {
    /// Read the backend settings from the process environment.
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).unwrap_or_default();
        Self {
            backend: var("ASSISTANT_CRYPTO_BACKEND"),
            fernet_keys: var("ASSISTANT_ENCRYPTION_KEY"),
            kms_key_id: var("ASSISTANT_KMS_KEY_ID"),
            aws_region: var("AWS_REGION"),
            kms_endpoint_url: var("ASSISTANT_KMS_ENDPOINT_URL"),
        }
    }
}

/// Backend failure: data problems (`NotConfigured`, i.e. Python raising
/// `AssistantNotConfigured`) versus operational problems (`Transport`, i.e.
/// Python letting the error propagate).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CryptoError {
    /// Undecryptable ciphertext or missing/bad configuration.
    #[error("assistant not configured: {0}")]
    NotConfigured(AssistantError),
    /// Operational failure (KMS auth, throttling, endpoint down, or a
    /// non-UTF-8 plaintext, which Python's unwrapped `.decode("utf-8")`
    /// would propagate as `UnicodeDecodeError`).
    #[error("crypto transport failed: {0}")]
    Transport(String),
}

fn not_configured(detail: impl Into<String>) -> CryptoError {
    CryptoError::NotConfigured(AssistantError::AssistantNotConfigured(detail.into()))
}

/// One KMS call failure: `code` models
/// `ClientError.response["Error"]["Code"]` and is empty for transport-level
/// (`BotoCoreError`) failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KmsTransportError {
    pub code: String,
    pub message: String,
}

impl KmsTransportError {
    /// Transport-level failure (no error code, like `BotoCoreError`).
    pub fn transport(message: impl Into<String>) -> Self {
        Self {
            code: String::new(),
            message: message.into(),
        }
    }

    /// API-level failure with an AWS error code (like `ClientError`).
    pub fn coded(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for KmsTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.code.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.code, self.message)
        }
    }
}

impl std::error::Error for KmsTransportError {}

/// Injectable KMS wire (`boto3.client("kms")`, `crypto.py:86-96`): region
/// and endpoint URL select the client at wiring time; the calls below are
/// the whole contract the backend relies on.
pub trait KmsTransport {
    /// `encrypt(KeyId, Plaintext)` → raw `CiphertextBlob`.
    fn encrypt(&self, key_id: &str, plaintext: &[u8]) -> Result<Vec<u8>, KmsTransportError>;
    /// `decrypt(CiphertextBlob, KeyId)` → `Plaintext`; `KeyId` is pinned so a
    /// ciphertext only decrypts under the expected CMK (`crypto.py:112-115`).
    fn decrypt(&self, key_id: &str, ciphertext: &[u8]) -> Result<Vec<u8>, KmsTransportError>;
    /// `re_encrypt(CiphertextBlob, DestinationKeyId)` → raw
    /// `CiphertextBlob` (`crypto.py:128`).
    fn re_encrypt(
        &self,
        ciphertext: &[u8],
        destination_key_id: &str,
    ) -> Result<Vec<u8>, KmsTransportError>;
}

/// AWS KMS backend (`crypto.py:57-131`): direct Encrypt/Decrypt over tiny
/// BYOK keys (no data-key envelope); `api_key_encrypted` holds the raw KMS
/// `CiphertextBlob`.
#[derive(Debug, Clone)]
pub struct KmsBackend {
    key_id: String,
    /// `AWS_REGION` (client selection; informational below this layer).
    pub region: String,
    /// `ASSISTANT_KMS_ENDPOINT_URL` (client selection; informational below).
    pub endpoint_url: String,
}

impl KmsBackend {
    /// Build from config; the id is stripped (`crypto.py:78`).
    pub fn from_config(config: &CryptoConfig) -> Self {
        Self {
            key_id: config.kms_key_id.trim().to_owned(),
            region: config.aws_region.trim().to_owned(),
            endpoint_url: config.kms_endpoint_url.trim().to_owned(),
        }
    }

    /// Whether the backend can encrypt/decrypt (`crypto.py:98-99`).
    pub fn is_configured(&self) -> bool {
        !self.key_id.is_empty()
    }

    fn require_key_id(&self) -> Result<&str, CryptoError> {
        if self.key_id.is_empty() {
            Err(not_configured(
                "ASSISTANT_KMS_KEY_ID is not set; BYOK keys cannot be stored.",
            ))
        } else {
            Ok(&self.key_id)
        }
    }

    /// Encrypt (`crypto.py:101-107`): every transport failure surfaces as
    /// `NotConfigured` (the ported wart — operational errors do not
    /// propagate here, unlike `decrypt`).
    pub fn encrypt(&self, plaintext: &str, kms: &dyn KmsTransport) -> Result<Vec<u8>, CryptoError> {
        let key_id = self.require_key_id()?.to_owned();
        kms.encrypt(&key_id, plaintext.as_bytes())
            .map_err(|err| not_configured(format!("KMS encrypt failed: {err}")))
    }

    /// Decrypt (`crypto.py:109-123`): empty tokens decode to `""` without a
    /// KMS call; undecryptable codes become `NotConfigured`; anything else
    /// propagates as [`CryptoError::Transport`].
    pub fn decrypt(&self, token: &[u8], kms: &dyn KmsTransport) -> Result<String, CryptoError> {
        if token.is_empty() {
            return Ok(String::new());
        }
        let key_id = self.require_key_id()?.to_owned();
        let plaintext = kms.decrypt(&key_id, token).map_err(|err| {
            if KMS_UNDECRYPTABLE_CODES.contains(&err.code.as_str()) {
                not_configured(
                    "Stored BYOK key could not be decrypted with the configured KMS key.",
                )
            } else {
                CryptoError::Transport(err.to_string())
            }
        })?;
        String::from_utf8(plaintext)
            .map_err(|err| CryptoError::Transport(format!("KMS plaintext is not UTF-8: {err}")))
    }

    /// Re-encrypt under the current key (`crypto.py:125-131`): every failure
    /// surfaces as `NotConfigured`.
    pub fn rotate(&self, token: &[u8], kms: &dyn KmsTransport) -> Result<Vec<u8>, CryptoError> {
        let key_id = self.require_key_id()?.to_owned();
        kms.re_encrypt(token, &key_id)
            .map_err(|err| not_configured(format!("KMS re-encrypt failed: {err}")))
    }
}

/// Local Fernet backend (`crypto.py:134-190`): app-managed keys, first
/// encrypts, all decrypt. Raw cipher work delegates to
/// [`FernetKeyring`](pidash_db::assistant::fernet_keys::FernetKeyring).
#[derive(Debug, Clone)]
pub struct FernetBackend {
    keys_raw: String,
}

impl FernetBackend {
    /// Build from config (`ASSISTANT_ENCRYPTION_KEY`).
    pub fn from_config(config: &CryptoConfig) -> Self {
        Self {
            keys_raw: config.fernet_keys.clone(),
        }
    }

    fn keyring(&self) -> Result<FernetKeyring, CryptoError> {
        match FernetKeyring::from_comma_list(&self.keys_raw) {
            Ok(ring) => Ok(ring),
            Err(FernetKeyError::NoKeys) => Err(not_configured(
                "ASSISTANT_ENCRYPTION_KEY is not set; BYOK keys cannot be stored.",
            )),
            Err(FernetKeyError::InvalidKey) | Err(FernetKeyError::InvalidToken) => Err(
                not_configured("ASSISTANT_ENCRYPTION_KEY must contain valid Fernet key(s)."),
            ),
        }
    }

    /// Whether the backend can encrypt/decrypt (`crypto.py:162-169`):
    /// non-empty key list that parses.
    pub fn is_configured(&self) -> bool {
        self.keyring().is_ok()
    }

    /// Encrypt under the first key (`crypto.py:171-172`).
    pub fn encrypt(&self, plaintext: &str) -> Result<Vec<u8>, CryptoError> {
        Ok(self.keyring()?.encrypt(plaintext.as_bytes()))
    }

    /// Decrypt under any key (`crypto.py:174-182`): empty tokens decode to
    /// `""` without touching the keyring.
    pub fn decrypt(&self, token: &[u8]) -> Result<String, CryptoError> {
        if token.is_empty() {
            return Ok(String::new());
        }
        let plaintext = self.keyring()?.decrypt(token).map_err(|_| {
            not_configured(
                "Stored BYOK key could not be decrypted with the configured encryption key.",
            )
        })?;
        String::from_utf8(plaintext)
            .map_err(|err| CryptoError::Transport(format!("Fernet plaintext is not UTF-8: {err}")))
    }

    /// Re-encrypt under the first key (`crypto.py:184-190`).
    pub fn rotate(&self, token: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.keyring()?.rotate(token).map_err(|_| {
            not_configured(
                "Stored BYOK key could not be decrypted with the configured encryption key.",
            )
        })
    }
}

/// Configured backend (`crypto.py:204-214`).
#[derive(Debug, Clone)]
pub enum Backend {
    /// `aws-kms` (also the unset/empty default).
    Kms(KmsBackend),
    /// `fernet`.
    Fernet(FernetBackend),
}

impl Backend {
    /// Select the backend (`get_backend`, `crypto.py:204-214`): unset or
    /// empty selects `aws-kms`; unknown names raise `NotConfigured` with the
    /// sorted registry in the message.
    pub fn from_config(config: &CryptoConfig) -> Result<Self, CryptoError> {
        let name = if config.backend.is_empty() {
            DEFAULT_BACKEND
        } else {
            config.backend.trim()
        };
        match name {
            BACKEND_AWS_KMS => Ok(Backend::Kms(KmsBackend::from_config(config))),
            BACKEND_FERNET => Ok(Backend::Fernet(FernetBackend::from_config(config))),
            _ => Err(not_configured(format!(
                "unknown ASSISTANT_CRYPTO_BACKEND '{name}' (available: {})",
                KNOWN_BACKENDS.join(", ")
            ))),
        }
    }
}

/// Backend configured (`crypto.py:220-221`).
pub fn is_configured(config: &CryptoConfig) -> bool {
    match Backend::from_config(config) {
        Ok(Backend::Kms(backend)) => backend.is_configured(),
        Ok(Backend::Fernet(backend)) => backend.is_configured(),
        Err(_) => false,
    }
}

/// Encrypt under the configured backend (`crypto.py:224-225`).
pub fn encrypt(
    config: &CryptoConfig,
    plaintext: &str,
    kms: &dyn KmsTransport,
) -> Result<Vec<u8>, CryptoError> {
    match Backend::from_config(config)? {
        Backend::Kms(backend) => backend.encrypt(plaintext, kms),
        Backend::Fernet(backend) => backend.encrypt(plaintext),
    }
}

/// Decrypt under the configured backend (`crypto.py:228-229`).
pub fn decrypt(
    config: &CryptoConfig,
    token: &[u8],
    kms: &dyn KmsTransport,
) -> Result<String, CryptoError> {
    match Backend::from_config(config)? {
        Backend::Kms(backend) => backend.decrypt(token, kms),
        Backend::Fernet(backend) => backend.decrypt(token),
    }
}

/// Re-encrypt under the configured backend (`crypto.py:232-233`).
pub fn rotate(
    config: &CryptoConfig,
    token: &[u8],
    kms: &dyn KmsTransport,
) -> Result<Vec<u8>, CryptoError> {
    match Backend::from_config(config)? {
        Backend::Kms(backend) => backend.rotate(token, kms),
        Backend::Fernet(backend) => backend.rotate(token),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::cell::RefCell;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/assistant/crypto.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    fn not_configured_code(err: &CryptoError) -> &str {
        match err {
            CryptoError::NotConfigured(assistant) => assistant.code(),
            other => panic!("expected NotConfigured, got {other:?}"),
        }
    }

    // Python-generated cross-compatibility vector (`cryptography` 3.4.8):
    // K1 encrypts b"sk-live-key-123".
    const K1: &str = "Lk0_5ciI2pVpujWpFeeysfbbGfRDTBMu-tNTJb7lLMw=";
    const K2: &str = "INrRUbePFWt0XS3zTLayhRBYicavxkDznAAxc8BBbr0=";
    const PYTHON_TOKEN: &[u8] = b"gAAAAABqu3DT7NWuBYgWhqKBzL4kSWPefHCTJIUedSSU8p2e_Kv8ey-DgaYGJgXY7cl_Pzb-NzdH2k30IhIapHseLGJHilF13w==";

    /// Recording fake: canned blobs, pinned-KeyId assertions, scripted errors.
    #[derive(Default)]
    struct FakeKms {
        key_ids_seen: RefCell<Vec<String>>,
        re_encrypt_key_ids_seen: RefCell<Vec<String>>,
        decrypt_calls: RefCell<usize>,
        decrypt_error: RefCell<Option<KmsTransportError>>,
    }

    impl KmsTransport for FakeKms {
        fn encrypt(&self, key_id: &str, plaintext: &[u8]) -> Result<Vec<u8>, KmsTransportError> {
            self.key_ids_seen.borrow_mut().push(key_id.to_owned());
            Ok([b"kms:", plaintext].concat())
        }

        fn decrypt(&self, key_id: &str, ciphertext: &[u8]) -> Result<Vec<u8>, KmsTransportError> {
            *self.decrypt_calls.borrow_mut() += 1;
            self.key_ids_seen.borrow_mut().push(key_id.to_owned());
            if let Some(err) = self.decrypt_error.borrow().clone() {
                return Err(err);
            }
            Ok([b"plain:", ciphertext].concat())
        }

        fn re_encrypt(
            &self,
            ciphertext: &[u8],
            destination_key_id: &str,
        ) -> Result<Vec<u8>, KmsTransportError> {
            self.re_encrypt_key_ids_seen
                .borrow_mut()
                .push(destination_key_id.to_owned());
            Ok([b"re:", ciphertext].concat())
        }
    }

    fn kms_config() -> CryptoConfig {
        CryptoConfig {
            backend: BACKEND_AWS_KMS.to_owned(),
            kms_key_id: "alias/test".to_owned(),
            ..CryptoConfig::default()
        }
    }

    fn fernet_config(keys: &str) -> CryptoConfig {
        CryptoConfig {
            backend: BACKEND_FERNET.to_owned(),
            fernet_keys: keys.to_owned(),
            ..CryptoConfig::default()
        }
    }

    #[test]
    fn registry_names_and_default_match_fixture() {
        let registry = &fixture()["backend_registry"];
        let backends: Vec<&str> = registry["backends"]
            .as_array()
            .expect("backends")
            .iter()
            .map(|name| name.as_str().expect("name"))
            .collect();
        assert_eq!(backends, [BACKEND_AWS_KMS, BACKEND_FERNET]);
        assert_eq!(DEFAULT_BACKEND, BACKEND_AWS_KMS);
        assert!(registry["default"]
            .as_str()
            .expect("default")
            .contains("aws-kms"));
    }

    #[test]
    fn unknown_backend_raises_not_configured() {
        let config = CryptoConfig {
            backend: "gcp-kms".to_owned(),
            ..CryptoConfig::default()
        };
        let err = Backend::from_config(&config).expect_err("unknown backend");
        assert_eq!(not_configured_code(&err), "assistant_not_configured");
        let detail = match err {
            CryptoError::NotConfigured(assistant) => assistant.detail().to_owned(),
            other => panic!("expected NotConfigured, got {other:?}"),
        };
        assert_eq!(
            detail,
            "unknown ASSISTANT_CRYPTO_BACKEND 'gcp-kms' (available: aws-kms, fernet)"
        );
        assert!(!is_configured(&config));
    }

    #[test]
    fn blank_backend_name_is_unknown() {
        // `(value or "aws-kms").strip()`: whitespace-only survives the `or`
        // and strips to "" -> unknown (crypto.py:207-212).
        let config = CryptoConfig {
            backend: "   ".to_owned(),
            ..CryptoConfig::default()
        };
        assert!(matches!(
            Backend::from_config(&config),
            Err(CryptoError::NotConfigured(_))
        ));
    }

    #[test]
    fn fernet_round_trip_matches_fixture() {
        let round_trip = &fixture()["fernet"]["round_trip"];
        let config = fernet_config(K1);
        assert!(is_configured(&config));
        let backend = FernetBackend::from_config(&config);
        assert!(backend.is_configured());
        let token = backend.encrypt("sk-test").expect("encrypts");
        assert!(!token.is_empty(), "ct_is_bytes");
        assert_eq!(round_trip["roundtrip"], true);
        assert_eq!(backend.decrypt(&token).expect("decrypts"), "sk-test");
        assert_eq!(backend.decrypt(b""), Ok(String::new()), "decrypt_empty");
        let rotated = backend.rotate(&token).expect("rotates");
        assert_eq!(round_trip["rotate_ok"], true);
        assert_eq!(backend.decrypt(&rotated).expect("decrypts"), "sk-test");
    }

    #[test]
    fn fernet_decrypts_python_token_after_prepend() {
        // Existing rows still decrypt after operators prepend a new key.
        let config = fernet_config(&format!("{K2},{K1}"));
        let backend = FernetBackend::from_config(&config);
        assert_eq!(
            backend
                .decrypt(PYTHON_TOKEN)
                .expect("python token decrypts"),
            "sk-live-key-123"
        );
        assert!(fixture()["fernet"]["key_prepend_still_decrypts_old"]
            .as_bool()
            .expect("flag"));
    }

    #[test]
    fn fernet_wrong_key_raises_not_configured() {
        let backend = FernetBackend::from_config(&fernet_config(K2));
        let err = backend.decrypt(PYTHON_TOKEN).expect_err("wrong key");
        assert_eq!(not_configured_code(&err), "assistant_not_configured");
        assert_eq!(
            fixture()["fernet"]["wrong_key_raises"]["code"],
            "assistant_not_configured"
        );
    }

    #[test]
    fn fernet_no_key_raises_not_configured() {
        let config = fernet_config("");
        assert!(!is_configured(&config));
        let backend = FernetBackend::from_config(&config);
        assert!(!backend.is_configured());
        let err = backend.encrypt("x").expect_err("no key");
        assert_eq!(not_configured_code(&err), "assistant_not_configured");
        let detail = match err {
            CryptoError::NotConfigured(assistant) => assistant.detail().to_owned(),
            other => panic!("expected NotConfigured, got {other:?}"),
        };
        assert!(detail.contains("ASSISTANT_ENCRYPTION_KEY is not set"));
    }

    #[test]
    fn fernet_invalid_key_is_not_configured() {
        let backend = FernetBackend::from_config(&fernet_config("not-a-key"));
        assert!(!backend.is_configured());
    }

    #[test]
    fn public_api_dispatches_by_backend() {
        let kms = FakeKms::default();
        let token = encrypt(&fernet_config(K1), "hello", &kms).expect("encrypts");
        assert_eq!(
            decrypt(&fernet_config(K1), &token, &kms).expect("decrypts"),
            "hello"
        );
        assert_eq!(decrypt(&fernet_config(K1), b"", &kms), Ok(String::new()));
        let rotated =
            rotate(&fernet_config(&format!("{K2},{K1}")), PYTHON_TOKEN, &kms).expect("rotates");
        assert_eq!(
            decrypt(&fernet_config(&format!("{K2},{K1}")), &rotated, &kms).expect("decrypts"),
            "sk-live-key-123"
        );
    }

    #[test]
    fn kms_unconfigured() {
        let unconfigured = &fixture()["aws_kms"]["unconfigured"];
        assert_eq!(unconfigured["configured"], false);
        let config = CryptoConfig {
            backend: BACKEND_AWS_KMS.to_owned(),
            ..CryptoConfig::default()
        };
        assert!(!is_configured(&config));
        let kms = FakeKms::default();
        let err = encrypt(&config, "x", &kms).expect_err("no key id");
        assert_eq!(not_configured_code(&err), "assistant_not_configured");
    }

    #[test]
    fn kms_encrypt_wraps_failures_as_not_configured() {
        // Ported wart: encrypt maps even operational failures to
        // NotConfigured (crypto.py:103-106).
        struct Failing;
        impl KmsTransport for Failing {
            fn encrypt(&self, _: &str, _: &[u8]) -> Result<Vec<u8>, KmsTransportError> {
                Err(KmsTransportError::coded("AccessDenied", "nope"))
            }
            fn decrypt(&self, _: &str, _: &[u8]) -> Result<Vec<u8>, KmsTransportError> {
                unreachable!()
            }
            fn re_encrypt(&self, _: &[u8], _: &str) -> Result<Vec<u8>, KmsTransportError> {
                unreachable!()
            }
        }
        let backend = KmsBackend::from_config(&kms_config());
        let err = backend.encrypt("x", &Failing).expect_err("fails");
        assert_eq!(not_configured_code(&err), "assistant_not_configured");
    }

    #[test]
    fn kms_decrypt_pins_key_id_and_maps_codes() {
        let undecryptable = fixture()["aws_kms"]["undecryptable_codes"]
            .as_array()
            .expect("codes")
            .iter()
            .map(|code| code.as_str().expect("code").to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            undecryptable,
            [
                "InvalidCiphertextException",
                "IncorrectKeyException",
                "NotFoundException"
            ]
        );
        assert_eq!(
            undecryptable,
            KMS_UNDECRYPTABLE_CODES
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );

        let backend = KmsBackend::from_config(&kms_config());
        // Empty token short-circuits without a KMS call (crypto.py:109-113).
        let kms = FakeKms::default();
        assert_eq!(backend.decrypt(b"", &kms), Ok(String::new()));
        assert_eq!(*kms.decrypt_calls.borrow(), 0);

        for code in undecryptable {
            let kms = FakeKms {
                decrypt_error: RefCell::new(Some(KmsTransportError::coded(code, "bad blob"))),
                ..FakeKms::default()
            };
            let err = backend.decrypt(b"blob", &kms).expect_err("undecryptable");
            assert_eq!(not_configured_code(&err), "assistant_not_configured");
            assert_eq!(kms.key_ids_seen.borrow().as_slice(), ["alias/test"]);
        }

        // Operational failures propagate (crypto.py:122, bare `raise`).
        let kms = FakeKms {
            decrypt_error: RefCell::new(Some(KmsTransportError::coded("AccessDenied", "denied"))),
            ..FakeKms::default()
        };
        assert!(matches!(
            backend.decrypt(b"blob", &kms),
            Err(CryptoError::Transport(_))
        ));
    }

    #[test]
    fn kms_rotate_re_encrypts_and_wraps_failures() {
        let backend = KmsBackend::from_config(&kms_config());
        let kms = FakeKms::default();
        let out = backend.rotate(b"blob", &kms).expect("rotates");
        assert_eq!(out, b"re:blob");
        assert_eq!(
            kms.re_encrypt_key_ids_seen.borrow().as_slice(),
            ["alias/test"]
        );

        struct Failing;
        impl KmsTransport for Failing {
            fn encrypt(&self, _: &str, _: &[u8]) -> Result<Vec<u8>, KmsTransportError> {
                unreachable!()
            }
            fn decrypt(&self, _: &str, _: &[u8]) -> Result<Vec<u8>, KmsTransportError> {
                unreachable!()
            }
            fn re_encrypt(&self, _: &[u8], _: &str) -> Result<Vec<u8>, KmsTransportError> {
                Err(KmsTransportError::transport("down"))
            }
        }
        let err = backend.rotate(b"blob", &Failing).expect_err("fails");
        assert_eq!(not_configured_code(&err), "assistant_not_configured");
    }
}
