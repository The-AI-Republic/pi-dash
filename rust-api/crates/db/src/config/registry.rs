//! The single catalog of where each config value is read from.
//!
//! Port of `apps/api/pi_dash/config/registry.py`. Every key the backend reads
//! through the accessor is declared here with a `source`:
//!
//! * [`ConfigSource::Env`] — read from the process environment (a local
//!   `.env` file or SSM-injected vars in the cloud; the code never
//!   distinguishes the two).
//! * [`ConfigSource::Db`] — read from the `instance_configurations` table,
//!   i.e. values an instance admin edits at runtime through the admin UI.
//!
//! A key belongs to exactly one source. Back-compat rule (OSS): every key the
//! legacy `get_configuration_value` resolver served from the DB stays `Db`,
//! so the registry — not a global flag — decides the source. The cloud flips
//! secret/identity keys to `Env` (SSM) through [`ENV_KEYS_OVERRIDE_VAR`], a
//! comma-separated list travelling the same path as the values it governs.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::value::ConfigValue;

/// Where a config key is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    Env,
    Db,
}

/// One registry row: source, fallback when unset/missing, and whether the
/// value is a secret (encrypted at rest in the DB, sensitive in the env).
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigEntry {
    pub source: ConfigSource,
    pub default: ConfigValue,
    pub secret: bool,
}

impl ConfigEntry {
    fn env(default: ConfigValue) -> Self {
        Self {
            source: ConfigSource::Env,
            default,
            secret: false,
        }
    }

    fn db(default: ConfigValue) -> Self {
        Self {
            source: ConfigSource::Db,
            default,
            secret: false,
        }
    }

    fn secret_env(default: ConfigValue) -> Self {
        Self {
            source: ConfigSource::Env,
            default,
            secret: true,
        }
    }

    fn secret_db(default: ConfigValue) -> Self {
        Self {
            source: ConfigSource::Db,
            default,
            secret: true,
        }
    }
}

/// Name of the env var listing keys to force to `Env` (comma-separated).
/// Read once when the process-global registry is first built, exactly like
/// Python reading it at import time.
pub const ENV_KEYS_OVERRIDE_VAR: &str = "PIDASH_CONFIG_ENV_KEYS";

/// The key catalog. Build with [`ConfigRegistry::build`] (reads
/// [`ENV_KEYS_OVERRIDE_VAR`]) or [`ConfigRegistry::build_with_overrides`]
/// for tests and the private overlay's programmatic reclassification.
#[derive(Debug, Clone)]
pub struct ConfigRegistry {
    entries: HashMap<String, ConfigEntry>,
}

impl ConfigRegistry {
    /// Build the registry, applying [`ENV_KEYS_OVERRIDE_VAR`] on top.
    pub fn build() -> Self {
        let raw = std::env::var(ENV_KEYS_OVERRIDE_VAR).unwrap_or_default();
        let overrides: HashMap<String, ConfigSource> = raw
            .split(',')
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(|k| (k.to_owned(), ConfigSource::Env))
            .collect();
        Self::build_with_overrides(overrides)
    }

    /// Build the registry with explicit per-key source overrides. Unknown
    /// keys are added as `Env` with a `Null` default, mirroring Python's
    /// `_build_config`.
    pub fn build_with_overrides(overrides: HashMap<String, ConfigSource>) -> Self {
        let mut entries = base_entries();
        for (key, source) in overrides {
            entries
                .entry(key)
                .and_modify(|e| e.source = source)
                .or_insert(ConfigEntry {
                    source,
                    default: ConfigValue::Null,
                    secret: false,
                });
        }
        Self { entries }
    }

    pub fn get(&self, key: &str) -> Option<&ConfigEntry> {
        self.entries.get(key)
    }

    pub fn is_registered(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    pub fn all_keys(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// The process-global registry, built once on first use (the Rust equivalent
/// of Python's import-time `CONFIG`). A private overlay that reclassifies
/// keys programmatically must call [`try_init_global`] before the first
/// accessor call; `build()` (env-var overrides) needs no pre-initialisation.
static GLOBAL: OnceLock<ConfigRegistry> = OnceLock::new();

/// The process-global registry, building it from the environment on first
/// use. Prefer passing an explicit `&ConfigRegistry` (the `_in` accessor
/// variants); this is the convenience path for call sites that mirror
/// Python's module-level `get_config` imports.
pub fn global() -> &'static ConfigRegistry {
    GLOBAL.get_or_init(ConfigRegistry::build)
}

/// Pre-install a registry (e.g. with overlay reclassifications) as the
/// process-global one. Fails when the global was already built.
pub fn try_init_global(registry: ConfigRegistry) -> Result<(), &'static str> {
    GLOBAL
        .set(registry)
        .map_err(|_| "global config registry already initialised")
}

/// Keys the legacy resolver managed, classified for back-compat: values that
/// `configure_instance` seeds and serves from the DB stay `Db`; analytics
/// keys that were never seeded (always fell through to env) are `Env`.
/// Mirrors `_RESOLVER_CONFIG` entry for entry.
fn resolver_entries(into: &mut HashMap<String, ConfigEntry>) {
    use ConfigValue as V;
    let db = ConfigEntry::db as fn(V) -> ConfigEntry;
    let env = ConfigEntry::env as fn(V) -> ConfigEntry;
    let sdb = ConfigEntry::secret_db as fn(V) -> ConfigEntry;
    let senv = ConfigEntry::secret_env as fn(V) -> ConfigEntry;
    let rows: &[(&str, ConfigEntry)] = &[
        // Authentication toggles (runtime, admin-editable).
        ("ENABLE_SIGNUP", db(V::from("1"))),
        ("ENABLE_EMAIL_PASSWORD", db(V::from("1"))),
        ("ENABLE_MAGIC_LINK_LOGIN", db(V::from("0"))),
        ("DISABLE_WORKSPACE_CREATION", db(V::from("0"))),
        // Google OAuth.
        ("GOOGLE_CLIENT_ID", db(V::Null)),
        ("GOOGLE_CLIENT_SECRET", sdb(V::Null)),
        ("ENABLE_GOOGLE_SYNC", db(V::from("0"))),
        // IS_*_ENABLED flags are derived + seeded by configure_instance.
        ("IS_GOOGLE_ENABLED", db(V::from("0"))),
        // GitHub OAuth.
        ("GITHUB_CLIENT_ID", db(V::Null)),
        ("GITHUB_CLIENT_SECRET", sdb(V::Null)),
        ("GITHUB_ORGANIZATION_ID", db(V::Null)),
        ("ENABLE_GITHUB_SYNC", db(V::from("0"))),
        ("IS_GITHUB_ENABLED", db(V::from("0"))),
        // Read from the env only (never seeded), so env-sourced.
        ("GITHUB_APP_NAME", env(V::Null)),
        // GitHub App: non-secret identity is db-sourced (admin-editable,
        // seeded from env); secrets are env-sourced (SSM in cloud).
        ("GITHUB_APP_ID", db(V::Null)),
        ("GITHUB_APP_SLUG", db(V::Null)),
        ("GITHUB_APP_CLIENT_ID", db(V::Null)),
        ("GITHUB_APP_PRIVATE_KEY", senv(V::Null)),
        ("GITHUB_APP_WEBHOOK_SECRET", senv(V::Null)),
        ("GITHUB_APP_CLIENT_SECRET", senv(V::Null)),
        // GitLab OAuth.
        ("GITLAB_HOST", db(V::Null)),
        ("GITLAB_CLIENT_ID", db(V::Null)),
        ("GITLAB_CLIENT_SECRET", sdb(V::Null)),
        ("ENABLE_GITLAB_SYNC", db(V::from("0"))),
        ("IS_GITLAB_ENABLED", db(V::from("0"))),
        // Gitea OAuth.
        ("IS_GITEA_ENABLED", db(V::from("0"))),
        ("GITEA_HOST", db(V::Null)),
        ("GITEA_CLIENT_ID", db(V::Null)),
        ("GITEA_CLIENT_SECRET", sdb(V::Null)),
        ("ENABLE_GITEA_SYNC", db(V::from("0"))),
        // SMTP / email.
        ("ENABLE_SMTP", db(V::from("0"))),
        ("EMAIL_HOST", db(V::from(""))),
        ("EMAIL_HOST_USER", db(V::from(""))),
        ("EMAIL_HOST_PASSWORD", sdb(V::from(""))),
        ("EMAIL_PORT", db(V::from("587"))),
        ("EMAIL_FROM", db(V::from(""))),
        ("EMAIL_USE_TLS", db(V::from("1"))),
        ("EMAIL_USE_SSL", db(V::from("0"))),
        // LLM.
        ("LLM_API_KEY", sdb(V::Null)),
        ("LLM_PROVIDER", db(V::from("openai"))),
        ("LLM_MODEL", db(V::from("gpt-4o-mini"))),
        // Deprecated, use LLM_MODEL.
        ("GPT_ENGINE", db(V::from("gpt-3.5-turbo"))),
        // Misc.
        ("UNSPLASH_ACCESS_KEY", sdb(V::from(""))),
        // Read from the env only (never seeded).
        ("SLACK_CLIENT_ID", env(V::Null)),
        // Analytics (never seeded; always env).
        ("POSTHOG_API_KEY", env(V::Null)),
        ("POSTHOG_HOST", env(V::Null)),
    ];
    into.extend(rows.iter().map(|(k, e)| ((*k).to_owned(), e.clone())));
}

/// Infrastructure / framework config read at boot. Always `Env`: it must
/// exist before the DB is reachable. Mirrors `_ENV_INFRA` entry for entry;
/// never overrides an already-declared resolver key.
fn infra_entries(into: &mut HashMap<String, ConfigEntry>) {
    use ConfigValue as V;
    let rows: &[(&str, ConfigValue)] = &[
        // Core / security.
        ("SECRET_KEY", V::Null),
        ("DEBUG", V::from("0")),
        ("ALLOWED_HOSTS", V::from("*")),
        ("CORS_ALLOWED_ORIGINS", V::from("")),
        // Database.
        ("DATABASE_URL", V::Null),
        ("POSTGRES_DB", V::Null),
        ("POSTGRES_USER", V::Null),
        ("POSTGRES_PASSWORD", V::Null),
        ("POSTGRES_HOST", V::Null),
        ("POSTGRES_PORT", V::from("5432")),
        ("ENABLE_READ_REPLICA", V::from("0")),
        ("DATABASE_READ_REPLICA_URL", V::Null),
        ("POSTGRES_READ_REPLICA_DB", V::Null),
        ("POSTGRES_READ_REPLICA_USER", V::Null),
        ("POSTGRES_READ_REPLICA_PASSWORD", V::Null),
        ("POSTGRES_READ_REPLICA_HOST", V::Null),
        ("POSTGRES_READ_REPLICA_PORT", V::from("5432")),
        // Redis.
        ("REDIS_URL", V::Null),
        ("REDIS_SOCKET_CONNECT_TIMEOUT", V::from(2.0)),
        ("REDIS_SOCKET_TIMEOUT", V::from(5.0)),
        ("REDIS_HEALTH_CHECK_INTERVAL", V::from(30)),
        ("REDIS_MAX_CONNECTIONS", V::Null),
        // AI assistant / KMS.
        ("ASSISTANT_CRYPTO_BACKEND", V::from("aws-kms")),
        ("ASSISTANT_KMS_KEY_ID", V::from("")),
        ("ASSISTANT_KMS_ENDPOINT_URL", V::from("")),
        ("ASSISTANT_ENCRYPTION_KEY", V::from("")),
        ("ASSISTANT_KEY_CACHE_TTL", V::from(300)),
        ("ASSISTANT_KEY_CACHE_MAXSIZE", V::from(1000)),
        ("ASSISTANT_BLOCK_PRIVATE_URLS", V::from("false")),
        ("ASSISTANT_TURN_SOFT_LIMIT", V::from(300)),
        ("ASSISTANT_TURN_HARD_LIMIT", V::from(330)),
        ("ASSISTANT_HISTORY_MAX_TURNS", V::from(40)),
        ("ASSISTANT_LOOP_HISTORY_MAX_TURNS", V::from(5)),
        // Self-managed GitLab hosts allowed for outbound calls.
        ("GITLAB_ALLOWED_HOSTS", V::from("")),
        // Loop (auto project management).
        ("LOOP_ENABLED", V::from("true")),
        ("LOOP_STAGGER_WINDOW_MINUTES", V::from(60)),
        ("LOOP_MAX_DISPATCH_PER_TICK", V::from(100)),
        ("LOOP_RECONCILE_EVERY_MINUTES", V::from(15)),
        ("LOOP_ROTATION_HEADROOM", V::from(30)),
        ("LOOP_MAX_WRITES", V::from(10)),
        ("LOOP_PR_LOOKUPS_PER_RUN", V::from(15)),
        // Storage / S3 / MinIO.
        ("USE_MINIO", V::from(0)),
        ("AWS_ACCESS_KEY_ID", V::from("access-key")),
        ("AWS_SECRET_ACCESS_KEY", V::from("secret-key")),
        ("AWS_S3_BUCKET_NAME", V::from("uploads")),
        ("AWS_REGION", V::from("")),
        ("AWS_S3_ENDPOINT_URL", V::Null),
        ("MINIO_ENDPOINT_URL", V::Null),
        ("MINIO_ENDPOINT_SSL", V::Null),
        ("SIGNED_URL_EXPIRATION", V::from("3600")),
        ("WEB_URL", V::Null),
        // RabbitMQ / Celery.
        ("RABBITMQ_HOST", V::from("localhost")),
        ("RABBITMQ_PORT", V::from("5672")),
        ("RABBITMQ_USER", V::from("guest")),
        ("RABBITMQ_PASSWORD", V::from("guest")),
        ("RABBITMQ_VHOST", V::from("/")),
        ("AMQP_URL", V::Null),
        // Misc product config.
        ("FILE_SIZE_LIMIT", V::from(5242880)),
        ("GITHUB_ACCESS_TOKEN", V::from(false)),
        ("GITHUB_SYNC_ENABLED", V::from("true")),
        ("SCHEDULER_ENABLED", V::from("true")),
        ("ANALYTICS_SECRET_KEY", V::from(false)),
        ("ANALYTICS_BASE_API", V::from(false)),
        // Runner transport / lifecycle tunables.
        ("LONG_POLL_INTERVAL_SECS", V::from(25)),
        ("ACCESS_TOKEN_TTL_SECS", V::from(3600)),
        ("RUNNER_OFFLINE_THRESHOLD_SECS", V::from(50)),
        ("OFFLINE_STREAM_TTL_SECS", V::from(86400)),
        ("OFFLINE_STREAM_MAXLEN", V::from(1000)),
        ("RUNNER_STREAM_MIN_RETENTION_SECS", V::from(3600)),
        ("EVENT_BATCH_MAX_AGE_MS", V::from(250)),
        ("EVENT_BATCH_MAX_BYTES", V::from(65536)),
        ("RUN_MESSAGE_DEDUPE_TTL_SECS", V::from(604800)),
        ("LATEST_RUNNER_VERSION", V::Null),
        ("MIN_RUNNER_VERSION", V::Null),
        ("RUNNER_AGENT_STALL_THRESHOLD_SECS", V::from(360)),
        ("RUNNER_AGENT_OBSERVABILITY_STALE_SECS", V::from(90)),
        // Desktop-bundled managed runner.
        ("MANAGED_RUNNER_ENABLED", V::from("false")),
        ("MANAGED_RUNNER_MAX_PER_USER_PROJECT", V::from(1)),
        ("MANAGED_RUNNER_QUEUED_MAX_AGE_SECS", V::from(43200)),
        ("MANAGED_RUNNER_GRACEFUL_STOP_SECS", V::from(30)),
        ("MANAGED_RUNNER_SWEEP_INTERVAL_SECONDS", V::from(300)),
        ("DESKTOP_MIN_VERSION_FOR_MANAGED_RUNNER", V::from("")),
        // In-house Cloud Agent executor.
        ("DEFAULT_AGENT_EXECUTOR", V::from("local_runner")),
        ("AGENT_RUN_TERMINAL_RECONCILE_INTERVAL_SECONDS", V::from(30)),
        ("CLOUD_AGENT_ENABLED", V::from("false")),
        ("CLOUD_AGENT_WRITES_ENABLED", V::from("false")),
        ("CLOUD_AGENT_GITHUB_TOOLS_ENABLED", V::from("true")),
        ("CLOUD_AGENT_DISABLED_TOOLS", V::from("")),
        ("CLOUD_AGENT_MODEL_REQUEST_TIMEOUT_SECONDS", V::from(60)),
        ("CLOUD_AGENT_EXECUTION_TIMEOUT_SECONDS", V::from(285)),
        ("CLOUD_AGENT_RUN_SOFT_LIMIT_SECONDS", V::from(300)),
        ("CLOUD_AGENT_RUN_HARD_LIMIT_SECONDS", V::from(330)),
        ("CLOUD_AGENT_STALE_GRACE_SECONDS", V::from(60)),
        ("CLOUD_AGENT_DISPATCH_LEASE_SECONDS", V::from(60)),
        ("CLOUD_AGENT_DISPATCH_BACKOFF_SECONDS", V::from(10)),
        ("CLOUD_AGENT_DISPATCH_SCAN_INTERVAL_SECONDS", V::from(10)),
        ("CLOUD_AGENT_SWEEP_INTERVAL_SECONDS", V::from(30)),
        ("CLOUD_AGENT_DISPATCH_SCAN_BATCH", V::from(100)),
        ("CLOUD_AGENT_MAX_QUEUE_AGE_SECONDS", V::from(900)),
        ("CLOUD_AGENT_MODEL_REQUEST_LIMIT", V::from(25)),
        ("CLOUD_AGENT_TOOL_CALL_LIMIT", V::from(20)),
        ("CLOUD_AGENT_WRITE_CALL_LIMIT", V::from(3)),
        ("CLOUD_AGENT_INPUT_TOKEN_LIMIT", V::from(144000)),
        ("CLOUD_AGENT_OUTPUT_TOKEN_LIMIT", V::from(16000)),
        ("CLOUD_AGENT_TOTAL_TOKEN_LIMIT", V::from(160000)),
        ("CLOUD_AGENT_MAX_OUTPUT_TOKENS_PER_REQUEST", V::from(4096)),
        ("CLOUD_AGENT_MAX_QUEUED_PER_WORKSPACE", V::from(20)),
        ("CLOUD_AGENT_MAX_RUNNING_PER_WORKSPACE", V::from(2)),
        ("CLOUD_AGENT_USER_CREATION_RATE_PER_MINUTE", V::from(6)),
        (
            "CLOUD_AGENT_WORKSPACE_CREATION_RATE_PER_MINUTE",
            V::from(30),
        ),
        ("CLOUD_AGENT_TOOL_TIMEOUT_SECONDS", V::from(20)),
        ("CLOUD_AGENT_MAX_TOOL_RESULT_BYTES", V::from(65536)),
        ("CLOUD_AGENT_MAX_PROMPT_BYTES", V::from(262144)),
        ("CLOUD_AGENT_MAX_FINAL_RESULT_BYTES", V::from(65536)),
        ("CLOUD_AGENT_MAX_EVENTS", V::from(500)),
        ("CLOUD_AGENT_BLOCK_PRIVATE_URLS", V::from("true")),
        // Sessions / cookies.
        ("SESSION_COOKIE_AGE", V::from(604800)),
        ("SESSION_COOKIE_NAME", V::from("session-id")),
        ("COOKIE_DOMAIN", V::Null),
        ("SESSION_SAVE_EVERY_REQUEST", V::from("0")),
        ("ADMIN_SESSION_COOKIE_AGE", V::from(3600)),
        // Base URLs.
        ("ADMIN_BASE_URL", V::Null),
        ("ADMIN_BASE_PATH", V::from("/god-mode/")),
        ("SPACE_BASE_URL", V::Null),
        ("SPACE_BASE_PATH", V::from("/spaces/")),
        ("APP_BASE_URL", V::Null),
        ("APP_BASE_PATH", V::from("/")),
        ("LIVE_BASE_URL", V::Null),
        ("LIVE_BASE_PATH", V::from("/live/")),
        ("HARD_DELETE_AFTER_DAYS", V::from(60)),
        ("INSTANCE_CHANGELOG_URL", V::from("")),
        ("ENABLE_DRF_SPECTACULAR", V::from("0")),
        // Mongo (legacy / optional).
        ("MONGO_DB_URL", V::from(false)),
        ("MONGO_DB_DATABASE", V::from(false)),
        // Production (Scout APM) + local.
        ("SCOUT_MONITOR", V::from(false)),
        ("SCOUT_KEY", V::from("")),
        (
            "EMAIL_BACKEND",
            V::from("django.core.mail.backends.smtp.EmailBackend"),
        ),
    ];
    for (key, default) in rows {
        into.entry((*key).to_owned()).or_insert(ConfigEntry {
            source: ConfigSource::Env,
            default: default.clone(),
            secret: false,
        });
    }
}

fn base_entries() -> HashMap<String, ConfigEntry> {
    let mut entries = HashMap::new();
    resolver_entries(&mut entries);
    infra_entries(&mut entries);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Snapshot of every key `configure_instance` seeds into
    /// `InstanceConfiguration` (36 from `instance_config_variables`, which
    /// already include `IS_GITEA_ENABLED`, plus the other 3 of the 4
    /// `DERIVED_FLAG_KEYS`), taken 2026-09-26. Mirrors
    /// `test_every_seeded_key_is_registered_as_db`: a seeded key routed to
    /// env would silently ignore its DB row.
    const SEEDED_KEYS: &[&str] = &[
        "DISABLE_WORKSPACE_CREATION",
        "EMAIL_FROM",
        "EMAIL_HOST",
        "EMAIL_HOST_PASSWORD",
        "EMAIL_HOST_USER",
        "EMAIL_PORT",
        "EMAIL_USE_SSL",
        "EMAIL_USE_TLS",
        "ENABLE_EMAIL_PASSWORD",
        "ENABLE_GITEA_SYNC",
        "ENABLE_GITHUB_SYNC",
        "ENABLE_GITLAB_SYNC",
        "ENABLE_GOOGLE_SYNC",
        "ENABLE_MAGIC_LINK_LOGIN",
        "ENABLE_SIGNUP",
        "ENABLE_SMTP",
        "GITEA_CLIENT_ID",
        "GITEA_CLIENT_SECRET",
        "GITEA_HOST",
        "GITHUB_APP_CLIENT_ID",
        "GITHUB_APP_ID",
        "GITHUB_APP_SLUG",
        "GITHUB_CLIENT_ID",
        "GITHUB_CLIENT_SECRET",
        "GITHUB_ORGANIZATION_ID",
        "GITLAB_CLIENT_ID",
        "GITLAB_CLIENT_SECRET",
        "GITLAB_HOST",
        "GOOGLE_CLIENT_ID",
        "GOOGLE_CLIENT_SECRET",
        "GPT_ENGINE",
        "IS_GITEA_ENABLED",
        "IS_GITHUB_ENABLED",
        "IS_GITLAB_ENABLED",
        "IS_GOOGLE_ENABLED",
        "LLM_API_KEY",
        "LLM_MODEL",
        "LLM_PROVIDER",
        "UNSPLASH_ACCESS_KEY",
    ];

    fn registry() -> ConfigRegistry {
        ConfigRegistry::build_with_overrides(HashMap::new())
    }

    #[test]
    fn every_entry_has_valid_source_and_shape() {
        let reg = registry();
        assert!(reg.len() > 150, "registry lost entries: {}", reg.len());
        for key in reg.all_keys() {
            let entry = reg.get(key).expect("listed key resolves");
            assert!(
                matches!(entry.source, ConfigSource::Env | ConfigSource::Db),
                "{key} has bad source",
            );
        }
    }

    #[test]
    fn seeded_keys_are_db_sourced() {
        let reg = registry();
        let bad: Vec<&&str> = SEEDED_KEYS
            .iter()
            .filter(|k| reg.get(k).map(|e| e.source) != Some(ConfigSource::Db))
            .collect();
        assert!(bad.is_empty(), "seeded keys not db-sourced: {bad:?}");
    }

    #[test]
    fn github_app_keys_classified_by_secret() {
        let reg = registry();
        for key in ["GITHUB_APP_ID", "GITHUB_APP_SLUG", "GITHUB_APP_CLIENT_ID"] {
            let entry = reg.get(key).expect(key);
            assert_eq!(entry.source, ConfigSource::Db, "{key}");
        }
        for key in [
            "GITHUB_APP_PRIVATE_KEY",
            "GITHUB_APP_WEBHOOK_SECRET",
            "GITHUB_APP_CLIENT_SECRET",
        ] {
            let entry = reg.get(key).expect(key);
            assert_eq!(entry.source, ConfigSource::Env, "{key}");
            assert!(entry.secret, "{key}");
        }
    }

    #[test]
    fn cloud_agent_has_no_platform_model_key() {
        assert!(!registry().is_registered("CLOUD_AGENT_MODEL_API_KEY"));
    }

    #[test]
    fn spot_check_defaults_match_python() {
        let reg = registry();
        let get = |k: &str| reg.get(k).expect(k).default.clone();
        assert_eq!(get("EMAIL_PORT"), ConfigValue::from("587"));
        assert_eq!(get("POSTHOG_API_KEY"), ConfigValue::Null);
        assert_eq!(get("FILE_SIZE_LIMIT"), ConfigValue::from(5242880));
        assert_eq!(get("REDIS_SOCKET_CONNECT_TIMEOUT"), ConfigValue::from(2.0));
        assert_eq!(get("GITHUB_ACCESS_TOKEN"), ConfigValue::from(false));
        assert_eq!(get("USE_MINIO"), ConfigValue::from(0));
        assert_eq!(get("LLM_MODEL"), ConfigValue::from("gpt-4o-mini"));
    }

    #[test]
    fn programmatic_overrides_reclassify() {
        let mut overrides = HashMap::new();
        overrides.insert("EMAIL_HOST".to_owned(), ConfigSource::Env);
        let reg = ConfigRegistry::build_with_overrides(overrides);
        assert_eq!(
            reg.get("EMAIL_HOST").expect("key").source,
            ConfigSource::Env
        );
        assert_eq!(
            reg.get("ENABLE_SIGNUP").expect("key").source,
            ConfigSource::Db
        );
    }

    #[test]
    fn unknown_override_keys_become_env_with_null_default() {
        let mut overrides = HashMap::new();
        overrides.insert("BRAND_NEW_KEY".to_owned(), ConfigSource::Env);
        let reg = ConfigRegistry::build_with_overrides(overrides);
        let entry = reg.get("BRAND_NEW_KEY").expect("key");
        assert_eq!(entry.source, ConfigSource::Env);
        assert_eq!(entry.default, ConfigValue::Null);
    }
}
