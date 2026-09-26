//! The single entry point for reading configuration values.
//!
//! Port of `apps/api/pi_dash/config/accessor.py`. Every config read goes
//! through a [`Resolver`] (or the `get_config` / `get_many` / `get_bool` /
//! `get_int` free functions over the process-global registry) rather than
//! touching the environment or the `instance_configurations` table directly.
//!
//! Two tiers, by necessity (same as Python):
//!
//! * `Env` keys resolve at any time, including process boot — they never
//!   touch the store.
//! * `Db` keys need a reachable database, so they can only be read at
//!   runtime. [`Settings`](super::settings::Settings) construction rejects
//!   `Db` keys loudly ([`ConfigError::DbAtBoot`]) rather than failing
//!   confusingly — boot code must only read `Env` keys.

use std::collections::HashMap;

use super::encryption::Keyring;
use super::registry::{global, ConfigRegistry, ConfigSource};
use super::value::ConfigValue;

/// Why a config lookup failed. Decrypt failures are NOT errors here — they
/// degrade to `""` inside [`Keyring`], matching Python.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// Strict mode (test harness) hit an unregistered key.
    #[error("unregistered config key {0:?} — register it in the config registry")]
    UnregisteredKey(String),
    /// A `Db`-sourced key was read where no database exists yet (boot).
    #[error(
        "db-sourced config key {0:?} read before the database was ready; \
         boot code must only read env-sourced keys"
    )]
    DbAtBoot(String),
    /// The store (database) failed.
    #[error("config store failed: {0}")]
    Store(String),
    /// A boot-time value had the wrong shape (e.g. an int setting holding
    /// `"abc"`). Python raises out of `int(...)` here; the boot fails either
    /// way.
    #[error("config key {key:?} must be {expected}, got {actual:?}")]
    TypeMismatch {
        key: String,
        expected: &'static str,
        actual: String,
    },
    /// The process-global registry was already initialised.
    #[error("global config registry already initialised")]
    AlreadyInitialised,
}

/// One `instance_configurations` row, as the accessor sees it.
/// (`instance_configurations`: `key` unique, `value` nullable text,
/// `category` text, `is_encrypted` bool.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigRow {
    pub value: Option<String>,
    pub is_encrypted: bool,
}

/// Where `Db`-tier reads come from. `get_many` batches into a single
/// `fetch`, mirroring Python's one-query `key__in` lookup.
///
/// The `Send` bound is load-bearing: resolvers run inside axum handlers.
pub trait ConfigStore {
    /// Fetch rows for `keys`. Missing keys are simply absent from the map
    /// (no row, not an error).
    fn fetch(
        &self,
        keys: &[&str],
    ) -> impl std::future::Future<Output = Result<HashMap<String, ConfigRow>, ConfigError>> + Send;
}

/// Postgres-backed store: one `SELECT` over `instance_configurations`.
/// The pool itself is built by F-04; this type only runs the config query.
pub struct PgConfigStore {
    pool: sqlx::PgPool,
}

impl PgConfigStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    /// The exact query text, visible for tests: keyed lookup over the
    /// `instance_configurations` table, nothing else.
    pub const QUERY: &'static str =
        "SELECT key, value, is_encrypted FROM instance_configurations WHERE key = ANY($1)";
}

impl ConfigStore for PgConfigStore {
    async fn fetch(&self, keys: &[&str]) -> Result<HashMap<String, ConfigRow>, ConfigError> {
        let rows: Vec<(String, Option<String>, bool)> = sqlx::query_as(Self::QUERY)
            .bind(keys)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| ConfigError::Store(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|(key, value, is_encrypted)| {
                (
                    key,
                    ConfigRow {
                        value,
                        is_encrypted,
                    },
                )
            })
            .collect())
    }
}

/// Whether to raise (vs. warn) on an unregistered key. Strict only when the
/// harness declares itself a test module via `DJANGO_SETTINGS_MODULE`
/// ending in `.test`, so CI catches unregistered keys; lenient everywhere
/// else so a forgotten key degrades gracefully to an env read. Reads the
/// environment directly to stay import-cycle free, exactly like Python.
fn is_strict() -> bool {
    std::env::var("DJANGO_SETTINGS_MODULE")
        .map(|v| v.ends_with(".test"))
        .unwrap_or(false)
}

/// Resolve one key through `registry` + `store` + `keyring`.
///
/// `default` optionally overrides the registry default for this call (used
/// by boot code keeping its inline fallbacks); it applies to env reads when
/// the var is unset and to db reads when no row exists.
pub async fn get_in<S: ConfigStore>(
    registry: &ConfigRegistry,
    store: &S,
    keyring: &Keyring,
    key: &str,
    default: Option<&ConfigValue>,
) -> Result<ConfigValue, ConfigError> {
    let Some(entry) = registry.get(key) else {
        return Ok(handle_unregistered(key, default));
    };
    match entry.source {
        ConfigSource::Env => Ok(read_env(key, &entry.default, default)),
        ConfigSource::Db => {
            let rows = store.fetch(std::slice::from_ref(&key)).await?;
            Ok(resolve_db_row(
                &entry.default,
                rows.get(key),
                default,
                keyring,
            ))
        }
    }
}

/// Resolve several keys at once, batching the db reads into one `fetch`.
/// Unregistered keys raise in strict mode via [`check_strict`], mirroring
/// Python's `get_many` (which routes unknown keys through
/// `_handle_unregistered`); only the legacy shim never raises.
pub async fn get_many_in<S: ConfigStore>(
    registry: &ConfigRegistry,
    store: &S,
    keyring: &Keyring,
    keys: &[&str],
) -> Result<HashMap<String, ConfigValue>, ConfigError> {
    let mut result = HashMap::with_capacity(keys.len());
    let mut db_keys = Vec::new();
    for key in keys {
        match registry.get(key) {
            None => {
                check_strict(key)?;
                result.insert((*key).to_owned(), handle_unregistered(key, None));
            }
            Some(entry) if entry.source == ConfigSource::Env => {
                result.insert((*key).to_owned(), read_env(key, &entry.default, None));
            }
            Some(_) => db_keys.push(*key),
        }
    }
    if !db_keys.is_empty() {
        let rows = store.fetch(&db_keys).await?;
        for key in db_keys {
            let entry = registry.get(key).expect("db key registered");
            // Encrypted rows decrypt here; a NULL row value is Null.
            let value = match rows.get(key) {
                None => entry.default.clone(),
                Some(row) => decrypt_row(row, keyring).unwrap_or(ConfigValue::Null),
            };
            result.insert(key.to_owned(), value);
        }
    }
    Ok(result)
}

/// Truthy iff the value is the string `"1"` (the project's convention).
/// Strict-aware: Python's `get_bool` delegates to `get_config`, which raises
/// on unregistered keys under the test harness.
pub async fn get_bool_in<S: ConfigStore>(
    registry: &ConfigRegistry,
    store: &S,
    keyring: &Keyring,
    key: &str,
) -> Result<bool, ConfigError> {
    Ok(get_strict_in(registry, store, keyring, key, None)
        .await?
        .is_flag_set())
}

/// Like `get_in` but coerced to int. With `default = None` the registry
/// default is used; a supplied default overrides the lookup AND is the
/// fallback when the value cannot be parsed.
pub async fn get_int_in<S: ConfigStore>(
    registry: &ConfigRegistry,
    store: &S,
    keyring: &Keyring,
    key: &str,
    default: Option<&ConfigValue>,
) -> Result<Option<i64>, ConfigError> {
    // Strict-aware like get_bool: Python's get_int delegates to get_config.
    let value = get_strict_in(registry, store, keyring, key, default).await?;
    // A supplied default is also the parse-failure fallback. (Python returns
    // the default object itself there, even when it is not an int; the typed
    // wrapper narrows that to `default.to_int()`, i.e. `None` for a default
    // that is not int-shaped either.)
    Ok(value
        .to_int()
        .or_else(|| default.and_then(ConfigValue::to_int)))
}

/// Read an env-tier key without a store. Used by boot code
/// ([`Settings`](super::settings::Settings)): rejects `Db` keys with
/// [`ConfigError::DbAtBoot`] so settings-time misuse fails loudly.
pub fn get_env_in(
    registry: &ConfigRegistry,
    key: &str,
    default: Option<&ConfigValue>,
) -> Result<ConfigValue, ConfigError> {
    get_env_with(registry, &|k| std::env::var(k).ok(), key, default)
}

/// Read an env-tier key from an explicit variable map instead of the process
/// environment. Same tier rule ([`ConfigError::DbAtBoot`]) and default
/// handling as [`get_env_in`]; [`Settings`](super::settings::Settings) builds
/// on this so its tests are hermetic and the private overlay can layer maps.
pub fn get_env_with(
    registry: &ConfigRegistry,
    vars: &dyn Fn(&str) -> Option<String>,
    key: &str,
    default: Option<&ConfigValue>,
) -> Result<ConfigValue, ConfigError> {
    if registry.get(key).is_none() {
        check_strict(key)?;
        tracing::warn!("unregistered config key {key:?}; defaulting to env");
        return Ok(vars(key)
            .map(ConfigValue::Str)
            .unwrap_or_else(|| default.cloned().unwrap_or(ConfigValue::Null)));
    }
    let entry = registry.get(key).expect("registered");
    if entry.source == ConfigSource::Db {
        return Err(ConfigError::DbAtBoot(key.to_owned()));
    }
    Ok(match vars(key) {
        Some(v) => ConfigValue::Str(v),
        None => default.unwrap_or(&entry.default).clone(),
    })
}

fn read_env(
    key: &str,
    registry_default: &ConfigValue,
    call_default: Option<&ConfigValue>,
) -> ConfigValue {
    match std::env::var(key) {
        Ok(v) => ConfigValue::Str(v),
        Err(_) => call_default.unwrap_or(registry_default).clone(),
    }
}

fn resolve_db_row(
    registry_default: &ConfigValue,
    row: Option<&ConfigRow>,
    call_default: Option<&ConfigValue>,
    keyring: &Keyring,
) -> ConfigValue {
    let fallback = || call_default.unwrap_or(registry_default).clone();
    match row {
        None => fallback(),
        Some(row) => match decrypt_row(row, keyring) {
            // A NULL row value is Null regardless of the encrypted flag:
            // Python's `_decrypt(None)` returns None as-is (NOT the default).
            None => ConfigValue::Null,
            Some(value) => value,
        },
    }
}

/// Decrypt a row's value. Returns `None` for NULL values; encrypted values
/// go through the keyring (failures degrade to `""` inside it).
fn decrypt_row(row: &ConfigRow, keyring: &Keyring) -> Option<ConfigValue> {
    let value = row.value.as_deref()?;
    if row.is_encrypted {
        Some(ConfigValue::Str(keyring.decrypt(value)))
    } else {
        Some(ConfigValue::Str(value.to_owned()))
    }
}

/// Unregistered keys: warn and fall back to a plain env read, exactly like
/// Python's lenient `_handle_unregistered` path. The strict (raising) path
/// lives in [`check_strict`], shared by [`get_strict_in`] and [`get_many_in`]:
/// only the legacy shim never raises, matching Python.
fn handle_unregistered(key: &str, default: Option<&ConfigValue>) -> ConfigValue {
    tracing::warn!("unregistered config key {key:?}; defaulting to env");
    match std::env::var(key) {
        Ok(v) => ConfigValue::Str(v),
        Err(_) => default.cloned().unwrap_or(ConfigValue::Null),
    }
}

/// Raise [`ConfigError::UnregisteredKey`] for an unknown key when the
/// harness is strict; otherwise pass. Single-key entry points
/// (`get_config`, boot reads) and [`get_many_in`] share this; only the
/// legacy shim never raises, matching Python.
fn check_strict(key: &str) -> Result<(), ConfigError> {
    if is_strict() {
        return Err(ConfigError::UnregisteredKey(key.to_owned()));
    }
    Ok(())
}

/// Like [`get_in`], but raises [`ConfigError::UnregisteredKey`] for unknown
/// keys when strict (the `get_config` contract).
pub async fn get_strict_in<S: ConfigStore>(
    registry: &ConfigRegistry,
    store: &S,
    keyring: &Keyring,
    key: &str,
    default: Option<&ConfigValue>,
) -> Result<ConfigValue, ConfigError> {
    if registry.get(key).is_none() {
        check_strict(key)?;
        return Ok(handle_unregistered(key, default));
    }
    get_in(registry, store, keyring, key, default).await
}

// --- global-registry convenience wrappers ---------------------------------
//
// These mirror Python's module-level `get_config` / `get_many` / `get_bool` /
// `get_int` imports: process-global registry, `Keyring::from_env()`.

/// Return the configured value for `key` (global registry; strict-aware).
pub async fn get_config<S: ConfigStore>(store: &S, key: &str) -> Result<ConfigValue, ConfigError> {
    get_strict_in(global(), store, &Keyring::from_env(), key, None).await
}

/// Return the configured value for `key`, overriding the registry default
/// for this call (global registry; strict-aware).
pub async fn get_config_with<S: ConfigStore>(
    store: &S,
    key: &str,
    default: ConfigValue,
) -> Result<ConfigValue, ConfigError> {
    get_strict_in(global(), store, &Keyring::from_env(), key, Some(&default)).await
}

/// Resolve several keys at once (global registry; strict-aware on unknown keys).
pub async fn get_many<S: ConfigStore>(
    store: &S,
    keys: &[&str],
) -> Result<HashMap<String, ConfigValue>, ConfigError> {
    get_many_in(global(), store, &Keyring::from_env(), keys).await
}

/// Truthy iff the value is the string `"1"` (global registry).
pub async fn get_bool<S: ConfigStore>(store: &S, key: &str) -> Result<bool, ConfigError> {
    get_bool_in(global(), store, &Keyring::from_env(), key).await
}

/// Coerce to int (global registry); `default` overrides and is the fallback.
pub async fn get_int<S: ConfigStore>(
    store: &S,
    key: &str,
    default: Option<&ConfigValue>,
) -> Result<Option<i64>, ConfigError> {
    get_int_in(global(), store, &Keyring::from_env(), key, default).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::env_lock;
    use crate::config::registry::ConfigRegistry;
    use std::collections::HashMap;

    /// In-memory store. Panics on `fetch` when `panic_on_fetch` is set, to
    /// prove env-tier reads never touch the database.
    #[derive(Default)]
    struct MemStore {
        rows: HashMap<String, ConfigRow>,
        fetches: std::sync::atomic::AtomicUsize,
        panic_on_fetch: bool,
    }

    impl ConfigStore for MemStore {
        async fn fetch(&self, keys: &[&str]) -> Result<HashMap<String, ConfigRow>, ConfigError> {
            if self.panic_on_fetch {
                panic!("env-tier read touched the database");
            }
            self.fetches
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(keys
                .iter()
                .filter_map(|k| self.rows.get(*k).map(|r| ((*k).to_owned(), r.clone())))
                .collect())
        }
    }

    fn mem(rows: &[(&str, Option<&str>, bool)]) -> MemStore {
        MemStore {
            rows: rows
                .iter()
                .map(|(k, v, e)| {
                    (
                        (*k).to_owned(),
                        ConfigRow {
                            value: v.map(str::to_owned),
                            is_encrypted: *e,
                        },
                    )
                })
                .collect(),
            ..Default::default()
        }
    }

    fn registry() -> ConfigRegistry {
        ConfigRegistry::build_with_overrides(HashMap::new())
    }

    fn keyring() -> Keyring {
        Keyring::from_secret("test-secret-key")
    }

    /// Run a future to completion without a tokio dependency in unit tests.
    fn block<F: std::future::Future>(f: F) -> F::Output {
        tokio_test_block_on(f)
    }

    fn tokio_test_block_on<F: std::future::Future>(f: F) -> F::Output {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("test runtime");
        rt.block_on(f)
    }

    // --- env tier ---------------------------------------------------------

    #[test]
    fn env_key_reads_from_environment() {
        let _g = env_lock();
        std::env::set_var("POSTHOG_API_KEY", "ph_from_env");
        let store = MemStore {
            panic_on_fetch: true,
            ..Default::default()
        };
        let v = block(get_in(
            &registry(),
            &store,
            &keyring(),
            "POSTHOG_API_KEY",
            None,
        ))
        .expect("read");
        assert_eq!(v, ConfigValue::from("ph_from_env"));
        std::env::remove_var("POSTHOG_API_KEY");
    }

    #[test]
    fn env_key_falls_back_to_registry_default() {
        let _g = env_lock();
        std::env::remove_var("POSTHOG_API_KEY");
        let store = MemStore {
            panic_on_fetch: true,
            ..Default::default()
        };
        let v = block(get_in(
            &registry(),
            &store,
            &keyring(),
            "POSTHOG_API_KEY",
            None,
        ))
        .expect("read");
        assert_eq!(v, ConfigValue::Null);
    }

    #[test]
    fn call_default_overrides_registry_default() {
        let _g = env_lock();
        std::env::remove_var("DEBUG");
        let store = MemStore {
            panic_on_fetch: true,
            ..Default::default()
        };
        // Registry default for DEBUG is "0"; the caller override wins.
        let v = block(get_in(
            &registry(),
            &store,
            &keyring(),
            "DEBUG",
            Some(&ConfigValue::from("fallback")),
        ))
        .expect("read");
        assert_eq!(v, ConfigValue::from("fallback"));
    }

    // --- db tier ----------------------------------------------------------

    #[test]
    fn db_key_reads_row() {
        let _g = env_lock();
        let store = mem(&[("EMAIL_HOST", Some("smtp.test"), false)]);
        let v = block(get_in(&registry(), &store, &keyring(), "EMAIL_HOST", None)).expect("read");
        assert_eq!(v, ConfigValue::from("smtp.test"));
    }

    #[test]
    fn db_key_missing_row_returns_default() {
        let _g = env_lock();
        let store = mem(&[]);
        let v = block(get_in(&registry(), &store, &keyring(), "EMAIL_PORT", None)).expect("read");
        assert_eq!(v, ConfigValue::from("587"));
    }

    #[test]
    fn db_secret_key_is_decrypted() {
        let _g = env_lock();
        let k = keyring();
        let token = k.encrypt("s3cret");
        let store = mem(&[("EMAIL_HOST_PASSWORD", Some(token.as_str()), true)]);
        let v = block(get_in(&registry(), &store, &k, "EMAIL_HOST_PASSWORD", None)).expect("read");
        assert_eq!(v, ConfigValue::from("s3cret"));
    }

    #[test]
    fn db_null_row_value_is_null() {
        let _g = env_lock();
        // A row with NULL value yields Null even when flagged encrypted
        // (Python's _decrypt(None) returns None as-is).
        let store = mem(&[("EMAIL_HOST", None, true)]);
        let v = block(get_in(&registry(), &store, &keyring(), "EMAIL_HOST", None)).expect("read");
        assert_eq!(v, ConfigValue::Null);
    }

    #[test]
    fn corrupt_secret_degrades_to_empty_string() {
        let _g = env_lock();
        let store = mem(&[("EMAIL_HOST_PASSWORD", Some("corrupt"), true)]);
        let v = block(get_in(
            &registry(),
            &store,
            &keyring(),
            "EMAIL_HOST_PASSWORD",
            None,
        ))
        .expect("read");
        assert_eq!(v, ConfigValue::from(""));
    }

    // --- batching ----------------------------------------------------------

    #[test]
    fn get_many_batches_db_reads_into_one_fetch() {
        let _g = env_lock();
        std::env::set_var("POSTHOG_HOST", "https://ph.test");
        let store = mem(&[("EMAIL_HOST", Some("smtp.test"), false)]);
        let out = block(get_many_in(
            &registry(),
            &store,
            &keyring(),
            &["EMAIL_HOST", "EMAIL_PORT", "POSTHOG_HOST"],
        ))
        .expect("read");
        assert_eq!(store.fetches.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(out.get("EMAIL_HOST"), Some(&ConfigValue::from("smtp.test")));
        assert_eq!(out.get("EMAIL_PORT"), Some(&ConfigValue::from("587")));
        assert_eq!(
            out.get("POSTHOG_HOST"),
            Some(&ConfigValue::from("https://ph.test"))
        );
        std::env::remove_var("POSTHOG_HOST");
    }

    // --- typed helpers -----------------------------------------------------

    #[test]
    fn bool_and_int_helpers() {
        let _g = env_lock();
        let store = mem(&[
            ("ENABLE_SIGNUP", Some("1"), false),
            ("ENABLE_SMTP", Some("0"), false),
        ]);
        assert!(block(get_bool_in(
            &registry(),
            &store,
            &keyring(),
            "ENABLE_SIGNUP"
        ))
        .expect("bool"));
        assert!(!block(get_bool_in(&registry(), &store, &keyring(), "ENABLE_SMTP")).expect("bool"));
        let store = mem(&[]);
        assert_eq!(
            block(get_int_in(
                &registry(),
                &store,
                &keyring(),
                "EMAIL_PORT",
                None
            ))
            .expect("int"),
            Some(587)
        );
    }

    #[test]
    fn int_invalid_returns_fallback() {
        let _g = env_lock();
        std::env::set_var("POSTHOG_HOST", "not-a-number");
        let store = MemStore {
            panic_on_fetch: true,
            ..Default::default()
        };
        let fb = ConfigValue::from(42);
        assert_eq!(
            block(get_int_in(
                &registry(),
                &store,
                &keyring(),
                "POSTHOG_HOST",
                Some(&fb)
            ))
            .expect("int"),
            Some(42)
        );
        std::env::remove_var("POSTHOG_HOST");
    }

    // --- unregistered keys --------------------------------------------------

    #[test]
    fn unregistered_key_strict_raises() {
        let _g = env_lock();
        std::env::set_var("DJANGO_SETTINGS_MODULE", "pi_dash.settings.test");
        std::env::set_var("WHATEVER_KEY", "x");
        let store = MemStore {
            panic_on_fetch: true,
            ..Default::default()
        };
        let err = block(get_strict_in(
            &registry(),
            &store,
            &keyring(),
            "WHATEVER_KEY",
            None,
        ))
        .expect_err("strict must raise");
        assert_eq!(err, ConfigError::UnregisteredKey("WHATEVER_KEY".to_owned()));
        std::env::remove_var("WHATEVER_KEY");
        std::env::remove_var("DJANGO_SETTINGS_MODULE");
    }

    #[test]
    fn typed_helpers_are_strict_aware() {
        // get_bool/get_int delegate to get_config in Python, so they raise
        // on unregistered keys under the test harness.
        let _g = env_lock();
        std::env::set_var("DJANGO_SETTINGS_MODULE", "pi_dash.settings.test");
        let store = MemStore::default();
        assert!(block(get_bool_in(&registry(), &store, &keyring(), "NOPE_BOOL")).is_err());
        assert!(block(get_int_in(
            &registry(),
            &store,
            &keyring(),
            "NOPE_INT",
            None
        ))
        .is_err());
        std::env::remove_var("DJANGO_SETTINGS_MODULE");
    }

    #[test]
    fn get_many_strict_raises_on_unregistered() {
        // Python's get_many routes unknown keys through _handle_unregistered,
        // which raises under the test harness.
        let _g = env_lock();
        std::env::set_var("DJANGO_SETTINGS_MODULE", "pi_dash.settings.test");
        let store = MemStore::default();
        let err = block(get_many_in(
            &registry(),
            &store,
            &keyring(),
            &["EMAIL_HOST", "WHATEVER_KEY"],
        ))
        .expect_err("strict get_many must raise");
        assert_eq!(err, ConfigError::UnregisteredKey("WHATEVER_KEY".to_owned()));
        std::env::remove_var("DJANGO_SETTINGS_MODULE");
    }

    #[test]
    fn unregistered_key_lenient_falls_back_to_env() {
        let _g = env_lock();
        std::env::remove_var("DJANGO_SETTINGS_MODULE");
        std::env::set_var("WHATEVER_KEY", "from_env");
        let store = MemStore {
            panic_on_fetch: true,
            ..Default::default()
        };
        let v = block(get_in(
            &registry(),
            &store,
            &keyring(),
            "WHATEVER_KEY",
            None,
        ))
        .expect("read");
        assert_eq!(v, ConfigValue::from("from_env"));
        std::env::remove_var("WHATEVER_KEY");
    }

    // --- boot tier rule ------------------------------------------------------

    #[test]
    fn boot_reads_reject_db_keys() {
        let _g = env_lock();
        let err = get_env_in(&registry(), "EMAIL_HOST", None).expect_err("db at boot");
        assert_eq!(err, ConfigError::DbAtBoot("EMAIL_HOST".to_owned()));
    }

    #[test]
    fn pg_store_targets_instance_configurations() {
        assert!(PgConfigStore::QUERY.contains("instance_configurations"));
        assert!(PgConfigStore::QUERY.contains("is_encrypted"));
    }
}
