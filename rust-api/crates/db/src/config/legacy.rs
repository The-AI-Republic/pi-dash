//! Compatibility shim over the per-key source registry.
//!
//! Port of `apps/api/pi_dash/license/utils/instance_value.py`'s
//! `get_configuration_value` (not `get_email_configuration`, which is
//! email-domain composition and travels with that domain's port). Old call
//! sites pass `[{key, default}]` pairs and get a tuple back; the source
//! (db vs env) now comes from the registry instead of the removed global
//! `SKIP_ENV_VAR` flag. New code prefers the accessor directly.
//!
//! Two deliberate differences from the accessor, both ported exactly:
//!
//! * the caller-supplied `default` is the ONLY fallback (registry defaults
//!   are ignored);
//! * unregistered keys read the environment without ever raising, even in
//!   strict mode.

use super::accessor::{ConfigError, ConfigRow, ConfigStore};
use super::encryption::Keyring;
use super::registry::{ConfigRegistry, ConfigSource};
use super::value::ConfigValue;

/// One legacy lookup: the key plus the caller's fallback.
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyItem {
    pub key: String,
    pub default: ConfigValue,
}

impl LegacyItem {
    pub fn new(key: impl Into<String>, default: ConfigValue) -> Self {
        Self {
            key: key.into(),
            default,
        }
    }
}

/// Resolve `items` in order, batching the db reads into one `fetch`.
/// Returns values positionally, like Python's tuple.
pub async fn get_configuration_values<S: ConfigStore>(
    registry: &ConfigRegistry,
    store: &S,
    keyring: &Keyring,
    items: &[LegacyItem],
) -> Result<Vec<ConfigValue>, ConfigError> {
    let mut db_keys = Vec::new();
    for item in items {
        if source_of(registry, &item.key) == ConfigSource::Db {
            db_keys.push(item.key.as_str());
        }
    }
    let rows = if db_keys.is_empty() {
        Default::default()
    } else {
        store.fetch(&db_keys).await?
    };
    Ok(items
        .iter()
        .map(|item| resolve_item(&item.key, &item.default, &rows, keyring, registry))
        .collect())
}

fn source_of(registry: &ConfigRegistry, key: &str) -> ConfigSource {
    registry
        .get(key)
        .map(|e| e.source)
        .unwrap_or(ConfigSource::Env)
}

fn resolve_item(
    key: &str,
    default: &ConfigValue,
    rows: &std::collections::HashMap<String, ConfigRow>,
    keyring: &Keyring,
    registry: &ConfigRegistry,
) -> ConfigValue {
    if source_of(registry, key) == ConfigSource::Db {
        match rows.get(key) {
            None => default.clone(),
            Some(row) => match row.value.as_deref() {
                // The legacy shim calls decrypt_data() directly, whose
                // falsy branch returns "" — so an encrypted NULL row yields
                // "", while a plain NULL row stays Null.
                None if row.is_encrypted => ConfigValue::Str(String::new()),
                None => ConfigValue::Null,
                Some(v) if row.is_encrypted => ConfigValue::Str(keyring.decrypt(v)),
                Some(v) => ConfigValue::Str(v.to_owned()),
            },
        }
    } else {
        match std::env::var(key) {
            Ok(v) => ConfigValue::Str(v),
            Err(_) => default.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::accessor::ConfigStore;
    use crate::config::env_lock;
    use std::collections::HashMap;

    #[derive(Default)]
    struct MemStore {
        rows: HashMap<String, ConfigRow>,
    }

    impl ConfigStore for MemStore {
        async fn fetch(&self, keys: &[&str]) -> Result<HashMap<String, ConfigRow>, ConfigError> {
            Ok(keys
                .iter()
                .filter_map(|k| self.rows.get(*k).map(|r| ((*k).to_owned(), r.clone())))
                .collect())
        }
    }

    fn registry() -> ConfigRegistry {
        ConfigRegistry::build_with_overrides(HashMap::new())
    }

    fn keyring() -> Keyring {
        Keyring::from_secret("test-secret-key")
    }

    fn block<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("test runtime")
            .block_on(f)
    }

    #[test]
    fn db_key_reads_row() {
        let _g = env_lock();
        let store = MemStore {
            rows: [(
                "EMAIL_HOST".to_owned(),
                ConfigRow {
                    value: Some("smtp.test".to_owned()),
                    is_encrypted: false,
                },
            )]
            .into(),
        };
        let out = block(get_configuration_values(
            &registry(),
            &store,
            &keyring(),
            &[LegacyItem::new("EMAIL_HOST", ConfigValue::from("fallback"))],
        ))
        .expect("read");
        assert_eq!(out, vec![ConfigValue::from("smtp.test")]);
    }

    #[test]
    fn db_key_missing_row_uses_caller_default_not_registry_default() {
        let _g = env_lock();
        let store = MemStore::default();
        let out = block(get_configuration_values(
            &registry(),
            &store,
            &keyring(),
            &[LegacyItem::new("EMAIL_HOST", ConfigValue::from("fallback"))],
        ))
        .expect("read");
        // Registry default for EMAIL_HOST is ""; the caller default wins.
        assert_eq!(out, vec![ConfigValue::from("fallback")]);
    }

    #[test]
    fn db_secret_is_decrypted() {
        let _g = env_lock();
        let k = keyring();
        let token = k.encrypt("shh");
        let store = MemStore {
            rows: [(
                "GOOGLE_CLIENT_SECRET".to_owned(),
                ConfigRow {
                    value: Some(token),
                    is_encrypted: true,
                },
            )]
            .into(),
        };
        let out = block(get_configuration_values(
            &registry(),
            &store,
            &k,
            &[LegacyItem::new("GOOGLE_CLIENT_SECRET", ConfigValue::Null)],
        ))
        .expect("read");
        assert_eq!(out, vec![ConfigValue::from("shh")]);
    }

    #[test]
    fn encrypted_null_row_yields_empty_string() {
        // The legacy shim calls decrypt_data() directly, whose falsy branch
        // returns "" — unlike the accessor, where NULL stays Null.
        let _g = env_lock();
        let store = MemStore {
            rows: [(
                "GOOGLE_CLIENT_SECRET".to_owned(),
                ConfigRow {
                    value: None,
                    is_encrypted: true,
                },
            )]
            .into(),
        };
        let out = block(get_configuration_values(
            &registry(),
            &store,
            &keyring(),
            &[LegacyItem::new(
                "GOOGLE_CLIENT_SECRET",
                ConfigValue::from("fallback"),
            )],
        ))
        .expect("read");
        assert_eq!(out, vec![ConfigValue::from("")]);
    }

    #[test]
    fn plain_null_row_yields_null() {
        let _g = env_lock();
        let store = MemStore {
            rows: [(
                "EMAIL_HOST".to_owned(),
                ConfigRow {
                    value: None,
                    is_encrypted: false,
                },
            )]
            .into(),
        };
        let out = block(get_configuration_values(
            &registry(),
            &store,
            &keyring(),
            &[LegacyItem::new("EMAIL_HOST", ConfigValue::from("fallback"))],
        ))
        .expect("read");
        assert_eq!(out, vec![ConfigValue::Null]);
    }

    #[test]
    fn env_key_reads_environment_and_default() {
        let _g = env_lock();
        std::env::set_var("POSTHOG_API_KEY", "ph_env");
        std::env::remove_var("POSTHOG_HOST");
        let store = MemStore::default();
        let out = block(get_configuration_values(
            &registry(),
            &store,
            &keyring(),
            &[
                LegacyItem::new("POSTHOG_API_KEY", ConfigValue::Null),
                LegacyItem::new("POSTHOG_HOST", ConfigValue::from("https://default")),
            ],
        ))
        .expect("read");
        assert_eq!(
            out,
            vec![
                ConfigValue::from("ph_env"),
                ConfigValue::from("https://default"),
            ]
        );
        std::env::remove_var("POSTHOG_API_KEY");
    }
}
