//! Creator key resolution for the model runtime (`llm.py:35-64`).
//!
//! [`KeyResolver`] is `get_decrypted_api_key`: the ciphertext row read
//! plus `crypto.decrypt` under the configured backend, served from a
//! short-lived in-process TTL+LRU cache. Two notes on the composition:
//!
//! * The ported [`KeyCache`][pidash_services::assistant::llm::KeyCache]
//!   takes an infallible decrypt closure, so a KMS failure would cache
//!   the fallback and serve it on later hits — Python retries the
//!   decrypt instead. [`FallibleKeyCache`] below mirrors the port's
//!   TTL+LRU line for line with a `Result`-carrying miss path, so
//!   failures propagate and cache nothing. (A `try_get` on the port
//!   would let this collapse back onto it.)
//! * In Python the decrypt runs inside `resolve_model_for_run`, before
//!   the `model_started` event; here the drive resolves from presence
//!   bits (foundation) and the decrypt runs inside `invoke_model`, so
//!   a decrypt-failed run keeps its `model_started` row. Accepted: the
//!   failure code and text still match.

#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pidash_services::assistant::crypto::{decrypt, CryptoConfig, CryptoError, KmsTransport};
use pidash_types::assistant::AssistantError;

/// `get_decrypted_api_key` failure: the `code` override plus the
/// message, exactly as `tasks.py:206` reads them off the raised
/// exception (`getattr(exc, "code", code)` over `classify_error`'s
/// text). `NotConfigured` carries the assistant code
/// (`assistant_not_configured`); operational failures carry none and
/// classify as `provider_error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyFailure {
    /// `exc.code` when the Python exception has one.
    pub code: Option<String>,
    /// `str(exc)`: the detail, or the class name when empty.
    pub message: String,
}

impl KeyFailure {
    fn not_configured(error: &AssistantError) -> Self {
        let detail = error.detail();
        Self {
            code: Some(error.code().to_owned()),
            message: if detail.is_empty() {
                error.class_name().to_owned()
            } else {
                detail.to_owned()
            },
        }
    }

    fn transport(message: String) -> Self {
        Self {
            code: None,
            message,
        }
    }
}

impl std::fmt::Display for KeyFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for KeyFailure {}

/// `crypto.decrypt` failure mapped onto [`KeyFailure`].
fn map_crypto_error(error: &CryptoError) -> KeyFailure {
    match error {
        CryptoError::NotConfigured(assistant) => KeyFailure::not_configured(assistant),
        CryptoError::Transport(message) => KeyFailure::transport(message.clone()),
    }
}

struct CacheEntry {
    plaintext: String,
    inserted: Instant,
}

/// The ported `KeyCache` eviction with a fallible miss path (see the
/// module docs): TTL (time) + LRU (capacity), keyed by ciphertext; a
/// miss just re-decrypts, so eviction is always safe — and a failed
/// decrypt stores nothing, so the next call retries like Python.
struct FallibleKeyCache {
    ttl: Duration,
    maxsize: usize,
    entries: HashMap<Vec<u8>, CacheEntry>,
    /// Insertion order, oldest first; a hit moves its key to the back.
    order: VecDeque<Vec<u8>>,
}

impl FallibleKeyCache {
    /// `_get_key_cache` (`llm.py:35-41`): both settings clamp to 1.
    fn new(ttl_secs: i64, maxsize: usize) -> Self {
        Self {
            ttl: Duration::from_secs(ttl_secs.max(1) as u64),
            maxsize: maxsize.max(1),
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// `get_decrypted_api_key`'s cache half: hit, expiring-miss, or
    /// decrypt-and-store. `decrypt` runs at most once per miss and its
    /// error propagates without storing.
    fn get_or_try_decrypt(
        &mut self,
        token: &[u8],
        decrypt: impl FnOnce() -> Result<String, KeyFailure>,
    ) -> Result<String, KeyFailure> {
        if let Some(entry) = self.entries.get(token) {
            if entry.inserted.elapsed() < self.ttl {
                let hit = entry.plaintext.clone();
                self.touch(token);
                return Ok(hit);
            }
        }
        let plaintext = decrypt()?;
        self.insert(token.to_owned(), plaintext.clone());
        Ok(plaintext)
    }

    fn touch(&mut self, token: &[u8]) {
        if let Some(pos) = self.order.iter().position(|k| k == token) {
            self.order.remove(pos);
        }
        self.order.push_back(token.to_owned());
    }

    fn insert(&mut self, token: Vec<u8>, plaintext: String) {
        if !self.entries.contains_key(&token) {
            while self.entries.len() >= self.maxsize {
                if let Some(oldest) = self.order.pop_front() {
                    self.entries.remove(&oldest);
                } else {
                    break;
                }
            }
            self.order.push_back(token.clone());
        }
        self.entries.insert(
            token,
            CacheEntry {
                plaintext,
                inserted: Instant::now(),
            },
        );
    }
}

/// `get_decrypted_api_key` (`llm.py:44-64`) over an explicit backend +
/// transport: direct decrypt when caching is disabled
/// (`ASSISTANT_KEY_CACHE_TTL <= 0`, the raw setting) or the token is
/// empty, else the shared in-process cache. Clones share the cache
/// (Python's is process-global).
pub struct KeyResolver {
    config: CryptoConfig,
    kms: Arc<dyn KmsTransport + Send + Sync>,
    cache: Arc<Mutex<FallibleKeyCache>>,
    cache_ttl_raw: i64,
}

impl KeyResolver {
    /// Build from the environment: [`CryptoConfig::from_env`], the
    /// production [`HttpKmsTransport`][pidash_api::assistant::kms::HttpKmsTransport],
    /// and the registry TTL/maxsize.
    pub fn from_env(ttl_secs: i64, maxsize: i64) -> Self {
        let config = CryptoConfig::from_env();
        let kms = pidash_api::assistant::kms::HttpKmsTransport::from_env(&config);
        Self::with_transport(config, Arc::new(kms), ttl_secs, maxsize)
    }

    /// Build over an explicit transport (tests inject a fake).
    pub fn with_transport(
        config: CryptoConfig,
        kms: Arc<dyn KmsTransport + Send + Sync>,
        ttl_secs: i64,
        maxsize: i64,
    ) -> Self {
        Self {
            config,
            kms,
            cache: Arc::new(Mutex::new(FallibleKeyCache::new(
                ttl_secs,
                usize::try_from(maxsize).unwrap_or(usize::MAX),
            ))),
            cache_ttl_raw: ttl_secs,
        }
    }

    /// Decrypt `token` (ciphertext bytes), cached per
    /// `get_decrypted_api_key`.
    pub fn decrypted_key(&self, token: &[u8]) -> Result<String, KeyFailure> {
        if self.cache_ttl_raw <= 0 || token.is_empty() {
            return self.decrypt(token);
        }
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        cache.get_or_try_decrypt(token, || self.decrypt(token))
    }

    fn decrypt(&self, token: &[u8]) -> Result<String, KeyFailure> {
        decrypt(&self.config, token, &*self.kms).map_err(|error| map_crypto_error(&error))
    }
}

impl Clone for KeyResolver {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            kms: self.kms.clone(),
            cache: self.cache.clone(),
            cache_ttl_raw: self.cache_ttl_raw,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_services::assistant::crypto::KmsTransportError;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// `FernetBackend` needs no KMS: any transport works, this one panics
    /// if touched so cache hits stay audible.
    struct PanicKms;

    impl KmsTransport for PanicKms {
        fn encrypt(&self, _key_id: &str, _plaintext: &[u8]) -> Result<Vec<u8>, KmsTransportError> {
            panic!("kms touched")
        }

        fn decrypt(&self, _key_id: &str, _ciphertext: &[u8]) -> Result<Vec<u8>, KmsTransportError> {
            panic!("kms touched")
        }

        fn re_encrypt(
            &self,
            _ciphertext: &[u8],
            _destination_key_id: &str,
        ) -> Result<Vec<u8>, KmsTransportError> {
            panic!("kms touched")
        }
    }

    struct CountingKms {
        calls: Arc<AtomicUsize>,
        result: Result<Vec<u8>, KmsTransportError>,
    }

    impl KmsTransport for CountingKms {
        fn encrypt(&self, _key_id: &str, _plaintext: &[u8]) -> Result<Vec<u8>, KmsTransportError> {
            panic!("encrypt untouched")
        }

        fn decrypt(&self, _key_id: &str, _ciphertext: &[u8]) -> Result<Vec<u8>, KmsTransportError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match &self.result {
                Ok(plaintext) => Ok(plaintext.clone()),
                Err(error) => Err(error.clone()),
            }
        }

        fn re_encrypt(
            &self,
            _ciphertext: &[u8],
            _destination_key_id: &str,
        ) -> Result<Vec<u8>, KmsTransportError> {
            panic!("re-encrypt untouched")
        }
    }

    fn kms_config() -> CryptoConfig {
        CryptoConfig {
            backend: "aws-kms".to_owned(),
            fernet_keys: String::new(),
            kms_key_id: "key".to_owned(),
            aws_region: "us-east-1".to_owned(),
            kms_endpoint_url: String::new(),
        }
    }

    #[test]
    fn cache_hit_skips_kms() {
        let calls = Arc::new(AtomicUsize::new(0));
        let kms = CountingKms {
            calls: calls.clone(),
            result: Ok(b"secret".to_vec()),
        };
        let resolver = KeyResolver::with_transport(kms_config(), Arc::new(kms), 300, 1000);
        assert_eq!(resolver.decrypted_key(b"token").expect("decrypt"), "secret");
        assert_eq!(resolver.decrypted_key(b"token").expect("cached"), "secret");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failures_cache_nothing_and_retry() {
        let calls = Arc::new(AtomicUsize::new(0));
        let kms = CountingKms {
            calls: calls.clone(),
            result: Err(KmsTransportError::coded("Boom", "down")),
        };
        let resolver = KeyResolver::with_transport(kms_config(), Arc::new(kms), 300, 1000);
        let failure = resolver.decrypted_key(b"token").expect_err("kms down");
        assert_eq!(failure.code, None);
        let failure = resolver
            .decrypted_key(b"token")
            .expect_err("kms still down");
        assert_eq!(failure.code, None);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn undecryptable_code_carries_assistant_code() {
        let calls = Arc::new(AtomicUsize::new(0));
        let kms = CountingKms {
            calls: calls.clone(),
            result: Err(KmsTransportError::coded(
                "InvalidCiphertextException",
                "nope",
            )),
        };
        let resolver = KeyResolver::with_transport(kms_config(), Arc::new(kms), 300, 1000);
        let failure = resolver.decrypted_key(b"token").expect_err("undecryptable");
        assert_eq!(failure.code.as_deref(), Some("assistant_not_configured"));
        assert!(!failure.message.is_empty());
    }

    #[test]
    fn disabled_cache_decrypts_every_time() {
        let calls = Arc::new(AtomicUsize::new(0));
        let kms = CountingKms {
            calls: calls.clone(),
            result: Ok(b"secret".to_vec()),
        };
        let resolver = KeyResolver::with_transport(kms_config(), Arc::new(kms), 0, 1000);
        assert_eq!(resolver.decrypted_key(b"token").expect("one"), "secret");
        assert_eq!(resolver.decrypted_key(b"token").expect("two"), "secret");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn empty_token_bypasses_cache_without_kms() {
        let resolver = KeyResolver::with_transport(kms_config(), Arc::new(PanicKms), 300, 1000);
        assert_eq!(resolver.decrypted_key(b"").expect("empty"), "");
    }

    #[test]
    fn clones_share_the_cache() {
        let calls = Arc::new(AtomicUsize::new(0));
        let kms = CountingKms {
            calls: calls.clone(),
            result: Ok(b"secret".to_vec()),
        };
        let resolver = KeyResolver::with_transport(kms_config(), Arc::new(kms), 300, 1000);
        let twin = KeyResolver {
            config: resolver.config.clone(),
            kms: Arc::new(PanicKms),
            cache: resolver.cache.clone(),
            cache_ttl_raw: 300,
        };
        assert_eq!(resolver.decrypted_key(b"token").expect("prime"), "secret");
        // The twin's transport would panic: a hit proves the shared cache.
        assert_eq!(twin.decrypted_key(b"token").expect("shared hit"), "secret");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn lru_evicts_oldest() {
        let mut cache = FallibleKeyCache::new(300, 2);
        let ok = |_: &[u8]| Ok::<_, KeyFailure>("p".to_owned());
        cache.get_or_try_decrypt(b"a", || ok(b"a")).expect("a");
        cache.get_or_try_decrypt(b"b", || ok(b"b")).expect("b");
        cache
            .get_or_try_decrypt(b"a", || panic!("a hits"))
            .expect("a hit");
        cache.get_or_try_decrypt(b"c", || ok(b"c")).expect("c");
        // `b` (least-recently-used) evicted; `a` and `c` hit.
        let mut misses = 0;
        cache
            .get_or_try_decrypt(b"b", || {
                misses += 1;
                ok(b"b")
            })
            .expect("b");
        assert_eq!(misses, 1);
        cache
            .get_or_try_decrypt(b"c", || panic!("c hits"))
            .expect("c");
    }
}
