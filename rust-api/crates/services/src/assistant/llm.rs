//! BYOK model resolution for the assistant (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/runtime/llm.py:1-115`: the short-lived
//! in-process cache of decrypted BYOK keys (`_get_key_cache`,
//! `get_decrypted_api_key`), the per-user config gates (`get_config`,
//! `resolve_byok_model`), the provider branches (`build_model`) and
//! `model_label`. Fixture id F-A6-08 (`rust-api/fixtures/assistant/runtime.json`).
//!
//! Shape notes:
//!
//! * The Django row fetch (`get_config`, `llm.py:67-68` — first
//!   `UserLLMConfig` for the user) and the KMS decrypt
//!   ([`pidash_db::assistant`](pidash_db::assistant)) stay with the handler
//!   layer, which owns database and network handles. This module ports the
//!   pure decision surface: gate order, branch selection, cache policy, and
//!   the label format.
//! * Failures surface as
//!   [`AssistantError::LlmConfigMissing`](pidash_types::assistant::errors::AssistantError)
//!   (`llm_config_missing` / 422), reusing the ported error taxonomy rather
//!   than redefining it.
//! * The cache key is the raw ciphertext bytes. Python keys by the SHA-256
//!   hex of the ciphertext (`llm.py:53`); the digest never leaves the
//!   process, so keying by the bytes themselves keeps the same invalidation
//!   (new ciphertext → miss) with no extra dependency.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use pidash_types::assistant::errors::AssistantError;

/// Anthropic provider kind (`models.py:179-181`, `ProviderKind.ANTHROPIC`).
pub const PROVIDER_ANTHROPIC: &str = "anthropic";
/// OpenAI-compatible provider kind (`ProviderKind.OPENAI_COMPATIBLE`).
pub const PROVIDER_OPENAI_COMPATIBLE: &str = "openai_compatible";

/// Default key-cache TTL, seconds (`llm.py:38`, `ASSISTANT_KEY_CACHE_TTL`).
pub const DEFAULT_KEY_CACHE_TTL_SECS: i64 = 300;
/// Default key-cache capacity (`llm.py:39`, `ASSISTANT_KEY_CACHE_MAXSIZE`).
pub const DEFAULT_KEY_CACHE_MAXSIZE: usize = 1000;

/// Missing-config gate (`llm.py:74-75`).
pub const MSG_NO_PROVIDER: &str = "No LLM provider is configured for this user.";
/// Missing-model gate (`llm.py:76-77`).
pub const MSG_NO_MODEL: &str = "No model name is configured.";
/// SSRF gate (`llm.py:83-84`).
pub const MSG_ENDPOINT_BLOCKED: &str = "The configured provider endpoint is not allowed.";

/// Resolved BYOK model (`build_model`, `llm.py:93-111`).
///
/// The transport (pydantic-ai's `AnthropicModel` / `OpenAIChatModel`) lives
/// with the handler layer; this descriptor carries exactly what the branches
/// select so the wiring cannot mix them up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelRef {
    /// `AnthropicModel(model_name, provider=AnthropicProvider(api_key))`
    /// (`llm.py:99-103`): base URL is ignored on this branch, even when set.
    Anthropic { model: String },
    /// `OpenAIChatModel(model_name, provider=OpenAIProvider(base_url,
    /// api_key))` (`llm.py:105-111`).
    OpenAICompatible { model: String, base_url: String },
}

/// Gate order plus branch selection (`resolve_byok_model` + `build_model`,
/// `llm.py:71-111`).
///
/// * `has_api_key` is `cfg is not None and cfg.has_api_key` (Python
///   truthiness over `api_key_encrypted`: `None` and empty bytes both read
///   as missing — see `has_api_key` in the ported models).
/// * `base_url_blocked` is the caller-computed SSRF verdict for
///   `cfg.base_url`; it is consulted only when `base_url` is non-empty
///   (`if cfg.base_url and ssrf.is_blocked(...)`, `llm.py:83`).
/// * Any provider kind other than `"anthropic"` falls into the
///   OpenAI-compatible branch (the bare `else` at `llm.py:105`).
pub fn resolve_byok_model(
    has_api_key: bool,
    model_name: &str,
    provider_kind: &str,
    base_url: &str,
    base_url_blocked: bool,
) -> Result<ModelRef, AssistantError> {
    if !has_api_key {
        return Err(AssistantError::LlmConfigMissing(MSG_NO_PROVIDER.to_owned()));
    }
    if model_name.is_empty() {
        return Err(AssistantError::LlmConfigMissing(MSG_NO_MODEL.to_owned()));
    }
    if !base_url.is_empty() && base_url_blocked {
        return Err(AssistantError::LlmConfigMissing(
            MSG_ENDPOINT_BLOCKED.to_owned(),
        ));
    }
    if provider_kind == PROVIDER_ANTHROPIC {
        Ok(ModelRef::Anthropic {
            model: model_name.to_owned(),
        })
    } else {
        Ok(ModelRef::OpenAICompatible {
            model: model_name.to_owned(),
            base_url: base_url.to_owned(),
        })
    }
}

/// `model_label` (`llm.py:114-115`): `"<provider_kind>:<model_name>"`.
pub fn model_label(provider_kind: &str, model_name: &str) -> String {
    format!("{provider_kind}:{model_name}")
}

/// Whether the key cache is consulted (`llm.py:51`).
///
/// Caching is disabled when `ASSISTANT_KEY_CACHE_TTL <= 0` (the raw setting,
/// before the `max(1, …)` clamp applied at cache creation).
pub fn cache_enabled(key_cache_ttl: i64) -> bool {
    key_cache_ttl > 0
}

struct CacheEntry {
    plaintext: String,
    inserted: Instant,
}

/// Short-lived in-process cache of decrypted BYOK keys (`llm.py:25-64`).
///
/// TTL (time) + LRU (capacity) eviction; a miss just re-decrypts, so
/// eviction is always safe. Decryption runs outside any lock: this type is
/// deliberately lock-free and `!Sync`, so the handler layer holds it behind
/// its own `Mutex` and the KMS round-trip never serializes concurrent
/// decrypts of different keys (`llm.py:58-60`).
pub struct KeyCache {
    ttl: Duration,
    maxsize: usize,
    entries: HashMap<Vec<u8>, CacheEntry>,
    /// Insertion order, oldest first; a hit moves its key to the back, so
    /// the front is always the least-recently-used key.
    order: VecDeque<Vec<u8>>,
}

impl KeyCache {
    /// Create the cache (`_get_key_cache`, `llm.py:35-41`): both settings
    /// clamp to a minimum of 1.
    pub fn new(key_cache_ttl_secs: i64, maxsize: usize) -> Self {
        Self {
            ttl: Duration::from_secs(key_cache_ttl_secs.max(1) as u64),
            maxsize: maxsize.max(1),
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// Decrypt `token` (ciphertext bytes), served from the cache
    /// (`get_decrypted_api_key`, `llm.py:44-64`).
    ///
    /// An empty token bypasses the cache and decrypts directly
    /// (`or not token`, `llm.py:51`); callers bypass the whole cache when
    /// [`cache_enabled`] is false. `decrypt` runs at most once per miss.
    pub fn get_or_decrypt(&mut self, token: &[u8], decrypt: impl FnOnce() -> String) -> String {
        if token.is_empty() {
            return decrypt();
        }
        if let Some(entry) = self.entries.get(token) {
            if entry.inserted.elapsed() < self.ttl {
                let hit = entry.plaintext.clone();
                self.touch(token);
                return hit;
            }
        }
        let plaintext = decrypt();
        self.insert(token.to_owned(), plaintext.clone());
        plaintext
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_defaults_match_python_settings() {
        assert_eq!(DEFAULT_KEY_CACHE_TTL_SECS, 300);
        assert_eq!(DEFAULT_KEY_CACHE_MAXSIZE, 1000);
    }

    #[test]
    fn cache_disabled_when_ttl_nonpositive() {
        assert!(cache_enabled(300));
        assert!(!cache_enabled(0));
        assert!(!cache_enabled(-5));
    }

    #[test]
    fn resolve_gate_no_key_has_exact_message_and_code() {
        let err = resolve_byok_model(false, "m", PROVIDER_OPENAI_COMPATIBLE, "", false)
            .expect_err("must fail without a key");
        assert_eq!(err.code(), "llm_config_missing");
        assert_eq!(err.http_status(), 422);
        assert_eq!(err.detail(), MSG_NO_PROVIDER);
        // `cfg is None` reads the same as "no key" (llm.py:74).
        let err_none = resolve_byok_model(false, "", PROVIDER_ANTHROPIC, "", false)
            .expect_err("none-config must fail");
        assert_eq!(err_none.detail(), MSG_NO_PROVIDER);
    }

    #[test]
    fn resolve_gate_empty_model_name() {
        let err = resolve_byok_model(true, "", PROVIDER_OPENAI_COMPATIBLE, "", false)
            .expect_err("must fail without a model name");
        assert_eq!(err.detail(), MSG_NO_MODEL);
    }

    #[test]
    fn resolve_gate_ssrf_blocked_base_url() {
        let err = resolve_byok_model(true, "m", PROVIDER_OPENAI_COMPATIBLE, "http://x/", true)
            .expect_err("blocked endpoint must fail");
        assert_eq!(err.detail(), MSG_ENDPOINT_BLOCKED);
        // Empty base_url skips the guard even when the verdict says blocked.
        assert!(resolve_byok_model(true, "m", PROVIDER_OPENAI_COMPATIBLE, "", true).is_ok());
        assert!(
            resolve_byok_model(true, "m", PROVIDER_OPENAI_COMPATIBLE, "http://x/", false).is_ok()
        );
    }

    #[test]
    fn build_anthropic_branch_ignores_base_url() {
        assert_eq!(
            resolve_byok_model(
                true,
                "claude-sonnet",
                PROVIDER_ANTHROPIC,
                "http://proxy/",
                false
            ),
            Ok(ModelRef::Anthropic {
                model: "claude-sonnet".to_owned()
            })
        );
    }

    #[test]
    fn build_openai_compatible_branch_carries_base_url() {
        assert_eq!(
            resolve_byok_model(
                true,
                "m",
                PROVIDER_OPENAI_COMPATIBLE,
                "https://api.example/v1",
                false
            ),
            Ok(ModelRef::OpenAICompatible {
                model: "m".to_owned(),
                base_url: "https://api.example/v1".to_owned(),
            })
        );
        // Bare-else port: unknown kinds fall to OpenAI-compatible (llm.py:105).
        assert!(matches!(
            resolve_byok_model(true, "m", "other", "https://api.example/v1", false),
            Ok(ModelRef::OpenAICompatible { .. })
        ));
    }

    #[test]
    fn model_label_shape_matches_fixture_vector() {
        assert_eq!(
            model_label(PROVIDER_OPENAI_COMPATIBLE, "gpt-4o"),
            "openai_compatible:gpt-4o"
        );
        assert_eq!(model_label(PROVIDER_ANTHROPIC, "m"), "anthropic:m");
    }

    #[test]
    fn key_cache_hit_serves_without_redecrypt() {
        let mut cache = KeyCache::new(300, 1000);
        let mut calls = 0;
        let first = cache.get_or_decrypt(b"cipher", || {
            calls += 1;
            "plain".to_owned()
        });
        let second = cache.get_or_decrypt(b"cipher", || {
            calls += 1;
            "plain".to_owned()
        });
        assert_eq!((first.as_str(), second.as_str()), ("plain", "plain"));
        assert_eq!(calls, 1, "second read must be a cache hit");
    }

    #[test]
    fn key_cache_new_ciphertext_misses() {
        let mut cache = KeyCache::new(300, 1000);
        cache.get_or_decrypt(b"old", || "plain-old".to_owned());
        // A changed key auto-invalidates (new ciphertext -> miss, llm.py:26-27).
        let hit = cache.get_or_decrypt(b"new", || "plain-new".to_owned());
        assert_eq!(hit, "plain-new");
    }

    #[test]
    fn key_cache_empty_token_bypasses_cache() {
        let mut cache = KeyCache::new(300, 1000);
        let mut calls = 0;
        for _ in 0..2 {
            let out = cache.get_or_decrypt(b"", || {
                calls += 1;
                "plain".to_owned()
            });
            assert_eq!(out, "plain");
        }
        assert_eq!(calls, 2, "empty token must decrypt every time");
    }

    #[test]
    fn key_cache_evicts_least_recently_used() {
        let mut cache = KeyCache::new(300, 2);
        let dec = |t: &[u8]| String::from_utf8_lossy(t).into_owned();
        cache.get_or_decrypt(b"a", || dec(b"a"));
        cache.get_or_decrypt(b"b", || dec(b"b"));
        cache.get_or_decrypt(b"a", || dec(b"a")); // touch a; b is now LRU
        cache.get_or_decrypt(b"c", || dec(b"c")); // evicts b
        let mut calls = 0;
        let out = cache.get_or_decrypt(b"b", || {
            calls += 1;
            dec(b"b")
        });
        assert_eq!((out.as_str(), calls), ("b", 1));
    }
}
