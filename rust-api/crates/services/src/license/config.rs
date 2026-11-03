//! License-domain configuration resolver (D-01 utils).
//!
//! Port of `apps/api/pi_dash/license/utils/instance_value.py`:
//!
//! * `get_configuration_value(keys)` (`:28-54`) — batch resolver over
//!   `[{key, default}]` pairs, returning values positionally (a tuple in
//!   Python, a `Vec` here). Implemented by the F-03 kernel legacy shim
//!   (`pidash_db::config::legacy::get_configuration_values`): one
//!   `filter(key__in=db_keys)` fetch, encrypted rows decrypted via
//!   `decrypt_data`, plain rows verbatim, missing rows falling back to the
//!   caller default, env-sourced keys read from the process environment at
//!   call time.
//! * `get_email_configuration()` (`:57-74`) — the 7-key email composition,
//!   ported here (the kernel deliberately leaves email-domain composition to
//!   the domain port).
//!
//! Source policy (`_source`, `:14-18`): a key registered in the central
//! `CONFIG` registry reads from its registered source; an unregistered key
//! reads the environment. The caller-supplied `default` is the ONLY fallback
//! — registry defaults are ignored — and `os.environ.get(name, default)` is
//! evaluated at call time.
//!
//! Ported bugs / inherited semantics (translate, don't redesign):
//!
//! * Unregistered keys resolve to `env` here, while
//!   `InstanceConfigurationSerializer` defaults them to `db`
//!   (`configuration.py:26`) — the asymmetry is real in Python and kept.
//! * The instances endpoint default for `ENABLE_SIGNUP` (`"0"`,
//!   `views/instance.py`) differs from the seed default (`"1"`, registry);
//!   the resolver passes caller defaults through verbatim and takes no side.
//! * Decrypt failures degrade to `""` (see [`super::encryption`]).

pub use pidash_db::config::legacy::{get_configuration_values, LegacyItem};
pub use pidash_db::config::{ConfigRegistry, ConfigRow, ConfigSource, ConfigStore, ConfigValue};
// NOTE: `config::ConfigError` (F-01 DbConfig failures) is a different type;
// the resolver errors with the accessor one below.
pub use pidash_db::config::accessor::ConfigError;

use super::encryption::Keyring;

/// The 7 email keys in `get_email_configuration` order
/// (`instance_value.py:57-74`).
pub const EMAIL_KEYS: [&str; 7] = [
    "EMAIL_HOST",
    "EMAIL_HOST_USER",
    "EMAIL_HOST_PASSWORD",
    "EMAIL_PORT",
    "EMAIL_USE_TLS",
    "EMAIL_USE_SSL",
    "EMAIL_FROM",
];

const EMAIL_FROM_DEFAULT: &str = "Team Pi Dash <team@airepublic.com>";

/// Where `key` is read from: its registry source, or `env` when unregistered
/// (port of `_source`, `instance_value.py:14-18`).
pub fn source_of(registry: &ConfigRegistry, key: &str) -> ConfigSource {
    registry
        .get(key)
        .map(|e| e.source)
        .unwrap_or(ConfigSource::Env)
}

/// Build the 7 email lookup items with defaults evaluated through `getenv`
/// at call time, mirroring Python's `os.environ.get(...)` defaults
/// (`instance_value.py:58-73`):
///
/// * `EMAIL_HOST`, `EMAIL_HOST_USER`, `EMAIL_HOST_PASSWORD` default to the
///   env value or `None` when unset;
/// * `EMAIL_PORT` defaults to the env value or integer `587`;
/// * `EMAIL_USE_TLS` / `EMAIL_USE_SSL` default to `"1"` / `"0"`;
/// * `EMAIL_FROM` defaults to `"Team Pi Dash <team@airepublic.com>"`.
///
/// The reader is generic over a `Send + Sync` closure so callers can inject a
/// map-backed stub in tests while the async composition below stays `Send`
/// (a bare `&dyn Fn` across the `await` made the future `!Send`).
pub fn email_items_with<F>(getenv: &F) -> Vec<LegacyItem>
where
    F: Fn(&str) -> Option<String> + Send + Sync + ?Sized,
{
    let env_or_null = |key: &str| match getenv(key) {
        Some(v) => ConfigValue::Str(v),
        None => ConfigValue::Null,
    };
    vec![
        LegacyItem::new("EMAIL_HOST", env_or_null("EMAIL_HOST")),
        LegacyItem::new("EMAIL_HOST_USER", env_or_null("EMAIL_HOST_USER")),
        LegacyItem::new("EMAIL_HOST_PASSWORD", env_or_null("EMAIL_HOST_PASSWORD")),
        LegacyItem::new(
            "EMAIL_PORT",
            getenv("EMAIL_PORT")
                .map(ConfigValue::Str)
                .unwrap_or(ConfigValue::Int(587)),
        ),
        LegacyItem::new(
            "EMAIL_USE_TLS",
            getenv("EMAIL_USE_TLS")
                .map(ConfigValue::Str)
                .unwrap_or(ConfigValue::Str("1".to_owned())),
        ),
        LegacyItem::new(
            "EMAIL_USE_SSL",
            getenv("EMAIL_USE_SSL")
                .map(ConfigValue::Str)
                .unwrap_or(ConfigValue::Str("0".to_owned())),
        ),
        LegacyItem::new(
            "EMAIL_FROM",
            getenv("EMAIL_FROM")
                .map(ConfigValue::Str)
                .unwrap_or(ConfigValue::Str(EMAIL_FROM_DEFAULT.to_owned())),
        ),
    ]
}

/// The 7 email lookup items with defaults read from the live process
/// environment (the `os.environ.get` call-time evaluation).
pub fn email_items() -> Vec<LegacyItem> {
    email_items_with(&|key| std::env::var(key).ok())
}

/// Port of `get_email_configuration` (`instance_value.py:57-74`): the 7-tuple
/// in [`EMAIL_KEYS`] order, each entry the decrypted DB row or the call-time
/// env/default fallback.
///
/// The returned future is `Send` (the `Sync` bounds below), so axum handlers
/// on the multi-threaded runtime can await it directly.
pub async fn get_email_configuration<S: ConfigStore + Sync>(
    store: &S,
    keyring: &Keyring,
    registry: &ConfigRegistry,
) -> Result<Vec<ConfigValue>, ConfigError> {
    get_email_configuration_with(store, keyring, registry, &|key| std::env::var(key).ok()).await
}

/// [`get_email_configuration`] with an injectable environment reader (call-time
/// defaults without touching the process environment; used by tests).
/// `Send + Sync` on the reader keeps the future `Send`.
pub async fn get_email_configuration_with<S, F>(
    store: &S,
    keyring: &Keyring,
    registry: &ConfigRegistry,
    getenv: &F,
) -> Result<Vec<ConfigValue>, ConfigError>
where
    S: ConfigStore + Sync,
    F: Fn(&str) -> Option<String> + Send + Sync + ?Sized,
{
    let items = email_items_with(getenv);
    get_configuration_values(registry, store, keyring, &items).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const TEST_SECRET: &str = "test-secret-key";

    #[derive(Default)]
    struct MemStore {
        rows: HashMap<String, ConfigRow>,
    }

    impl MemStore {
        fn with(rows: Vec<(&str, Option<&str>, bool)>) -> Self {
            Self {
                rows: rows
                    .into_iter()
                    .map(|(k, v, enc)| {
                        (
                            k.to_owned(),
                            ConfigRow {
                                value: v.map(str::to_owned),
                                is_encrypted: enc,
                            },
                        )
                    })
                    .collect(),
            }
        }
    }

    impl ConfigStore for MemStore {
        fn fetch(
            &self,
            keys: &[&str],
        ) -> impl std::future::Future<Output = Result<HashMap<String, ConfigRow>, ConfigError>> + Send
        {
            let rows: HashMap<String, ConfigRow> = keys
                .iter()
                .filter_map(|k| self.rows.get(*k).map(|r| ((*k).to_owned(), r.clone())))
                .collect();
            async move { Ok(rows) }
        }
    }

    fn registry() -> ConfigRegistry {
        ConfigRegistry::build_with_overrides(HashMap::new())
    }

    fn keyring() -> Keyring {
        Keyring::from_secret(TEST_SECRET)
    }

    fn resolve(store: &MemStore, items: &[LegacyItem]) -> Vec<ConfigValue> {
        // The kernel resolver is async (DB fetch); tests block on it with a
        // throwaway current-thread runtime (tokio dev-dependency, added by
        // the wiring issue that declares `pub mod license;`).
        tokio_test_block_on(get_configuration_values(
            &registry(),
            store,
            &keyring(),
            items,
        ))
        .expect("test resolver")
    }

    fn tokio_test_block_on<F>(f: F) -> F::Output
    where
        F: std::future::Future,
    {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("test runtime")
            .block_on(f)
    }

    #[test]
    fn source_policy_matches_python() {
        // Fixture source_policy: registered keys use their registry source,
        // unregistered keys fall back to env (instance_value.py:14-18).
        let r = registry();
        assert_eq!(source_of(&r, "EMAIL_HOST"), ConfigSource::Db);
        assert_eq!(source_of(&r, "ENABLE_SIGNUP"), ConfigSource::Db);
        assert_eq!(source_of(&r, "GITHUB_APP_NAME"), ConfigSource::Env);
        assert_eq!(source_of(&r, "NO_SUCH_KEY"), ConfigSource::Env);
    }

    #[test]
    fn db_plain_missing_and_env_branches() {
        // Fixture resolver: plain rows verbatim (:50), missing rows yield the
        // caller default verbatim (:45-46), env-sourced keys read the env at
        // call time (:52) — all positionally in input order.
        let store = MemStore::with(vec![("ENABLE_SIGNUP", Some("1"), false)]);
        let out = resolve(
            &store,
            &[
                LegacyItem::new("ENABLE_SIGNUP", ConfigValue::Str("0".to_owned())),
                LegacyItem::new("GITHUB_CLIENT_ID", ConfigValue::Null),
            ],
        );
        assert_eq!(
            out,
            vec![ConfigValue::Str("1".to_owned()), ConfigValue::Null,]
        );
    }

    #[test]
    fn caller_default_wins_over_registry_default() {
        // The caller default is the ONLY fallback: ENABLE_SIGNUP's registry
        // default is "1", but a missing row yields the caller's "0" (the
        // instances-endpoint default, views/instance.py).
        let store = MemStore::with(vec![]);
        let out = resolve(
            &store,
            &[LegacyItem::new(
                "ENABLE_SIGNUP",
                ConfigValue::Str("0".to_owned()),
            )],
        );
        assert_eq!(out, vec![ConfigValue::Str("0".to_owned())]);
    }

    #[test]
    fn encrypted_rows_decrypt_and_null_rows_degrade() {
        // Encrypted rows decrypt via decrypt_data (:47-48); an encrypted NULL
        // row yields "" (the legacy shim calls decrypt_data directly, whose
        // falsy branch returns ""); a plain NULL row stays Null.
        let k = keyring();
        let token = k.encrypt("s3cr3t-value");
        let store = MemStore::with(vec![
            ("GITHUB_CLIENT_SECRET", Some(&token), true),
            ("GITHUB_CLIENT_ID", None, true),
            ("GITLAB_HOST", None, false),
        ]);
        let out = resolve(
            &store,
            &[
                LegacyItem::new("GITHUB_CLIENT_SECRET", ConfigValue::Null),
                LegacyItem::new("GITHUB_CLIENT_ID", ConfigValue::Null),
                LegacyItem::new("GITLAB_HOST", ConfigValue::Null),
            ],
        );
        assert_eq!(
            out,
            vec![
                ConfigValue::Str("s3cr3t-value".to_owned()),
                ConfigValue::Str(String::new()),
                ConfigValue::Null,
            ]
        );
    }

    #[test]
    fn env_keys_read_env_and_ignore_db() {
        // Env-sourced keys never consult the DB, even when a row exists.
        let store = MemStore::with(vec![("GITHUB_APP_NAME", Some("db-value"), false)]);
        let out = resolve(
            &store,
            &[LegacyItem::new(
                "GITHUB_APP_NAME",
                ConfigValue::Str("fallback".to_owned()),
            )],
        );
        // GITHUB_APP_NAME is unset in the test process environment, so the
        // caller default applies (os.environ.get(name, default)).
        assert_eq!(out, vec![ConfigValue::Str("fallback".to_owned())]);
    }

    #[test]
    fn email_items_have_call_time_defaults_in_order() {
        // Fixture email_configuration.golden.json: 7 keys in order with
        // call-time defaults (PORT 587, TLS "1", SSL "0", FROM team address,
        // the rest None when the env is empty).
        let empty: HashMap<String, String> = HashMap::new();
        let items = email_items_with(&|k| empty.get(k).cloned());
        assert_eq!(
            items.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(),
            EMAIL_KEYS
        );
        let defaults: Vec<ConfigValue> = items.into_iter().map(|i| i.default).collect();
        assert_eq!(
            defaults,
            vec![
                ConfigValue::Null,
                ConfigValue::Null,
                ConfigValue::Null,
                ConfigValue::Int(587),
                ConfigValue::Str("1".to_owned()),
                ConfigValue::Str("0".to_owned()),
                ConfigValue::Str("Team Pi Dash <team@airepublic.com>".to_owned()),
            ]
        );
    }

    #[test]
    fn email_items_pick_up_env_at_call_time() {
        // os.environ.get defaults are evaluated per call: a set EMAIL_PORT
        // (string, as env values always are) shadows the 587 default.
        let mut env = HashMap::new();
        env.insert("EMAIL_PORT".to_owned(), "2525".to_owned());
        let items = email_items_with(&|k| env.get(k).cloned());
        assert_eq!(items[3].default, ConfigValue::Str("2525".to_owned()));
    }

    #[test]
    fn email_configuration_future_is_send() {
        // The futures once held a bare `&dyn Fn` reader across an await,
        // making them `!Send` and unusable in multi-threaded axum handlers
        // (found by the PIDASHCONV-123 review). This fails to compile if the
        // future ever stops being `Send`.
        fn assert_send<T: Send>(_: T) {}
        let store = MemStore::default();
        let keyring = keyring();
        let registry = registry();
        assert_send(get_email_configuration(&store, &keyring, &registry));
        let empty: HashMap<String, String> = HashMap::new();
        assert_send(get_email_configuration_with(
            &store,
            &keyring,
            &registry,
            &|key: &str| empty.get(key).cloned(),
        ));
    }

    #[test]
    fn email_configuration_resolves_rows_then_fallbacks() {
        // End to end through the kernel: DB rows (decrypted when encrypted)
        // win; missing rows fall back to the call-time defaults.
        let k = keyring();
        let token = k.encrypt("mail-secret");
        let store = MemStore::with(vec![
            ("EMAIL_HOST", Some("smtp.example.com"), false),
            ("EMAIL_HOST_PASSWORD", Some(&token), true),
        ]);
        let empty: HashMap<String, String> = HashMap::new();
        let out = tokio_test_block_on(get_email_configuration_with(
            &store,
            &k,
            &registry(),
            &|key| empty.get(key).cloned(),
        ))
        .expect("email resolves");
        assert_eq!(
            out,
            vec![
                ConfigValue::Str("smtp.example.com".to_owned()),
                ConfigValue::Null,
                ConfigValue::Str("mail-secret".to_owned()),
                ConfigValue::Int(587),
                ConfigValue::Str("1".to_owned()),
                ConfigValue::Str("0".to_owned()),
                ConfigValue::Str("Team Pi Dash <team@airepublic.com>".to_owned()),
            ]
        );
    }
}
