//! Boot-time settings: the typed equivalent of Django's settings modules.
//!
//! Port of `apps/api/pi_dash/settings/common.py` (+ the `local.py` /
//! `production.py` / `test.py` overlays) for the values a Rust process needs
//! at boot. Django-framework furniture (`INSTALLED_APPS`, `MIDDLEWARE`,
//! `CACHES`, `LOGGING`, log-directory creation) has no Rust equivalent and
//! is out of scope by construction; everything here is runtime configuration
//! the server, worker or handlers actually consume.
//!
//! Layering, mirroring the Python modules:
//!
//! 1. `common.py` → [`Settings::from_env`]: every field resolves through the
//!    accessor ([`get_env_with`](super::accessor::get_env_with)), so the
//!    registry stays the single catalog and `Db`-tier keys are rejected at
//!    boot with [`ConfigError::DbAtBoot`].
//! 2. `local.py` / `production.py` / `test.py` → [`Profile`]: the small
//!    per-environment deltas (debug default, secure-proxy header, email
//!    backend, test seed defaults).
//! 3. the private overlay → [`SettingsOverlay`]: reclassify keys to `Env`
//!    (the programmatic form of `PIDASH_CONFIG_ENV_KEYS`) and mutate the
//!    resolved `Settings`, composed in the binary's `main` before the app
//!    builder runs.

use std::collections::HashMap;

use super::accessor::{get_env_with, ConfigError};
use super::registry::{ConfigRegistry, ConfigSource};
use super::value::ConfigValue;

/// Which settings overlay applies on top of the common resolution.
/// Mirrors `local.py` / `production.py` / `test.py`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    /// Plain `common.py` semantics.
    #[default]
    Common,
    /// `local.py`: debug on, console email backend unless overridden.
    Local,
    /// `production.py`: `DEBUG = int(...) == 1`, secure-proxy header on.
    Production,
    /// `test.py`: debug on, locmem email backend, localhost seed defaults
    /// for `WEB_URL` / `APP_BASE_URL` when unset.
    Test,
}

/// Hook for the private crate, applied in `main` before the app builder.
/// `env_keys` forces keys to `Env` (the programmatic form of the cloud's
/// `PIDASH_CONFIG_ENV_KEYS` SSM seam); `apply` mutates the resolved
/// settings (e.g. cloud-only tunables). The default impl is a no-op.
pub trait SettingsOverlay {
    fn env_keys(&self) -> &[&str] {
        &[]
    }

    fn apply(&self, _settings: &mut Settings) {}
}

/// No-op overlay for OSS boot.
pub struct NoOverlay;

impl SettingsOverlay for NoOverlay {}

const SMTP_BACKEND: &str = "django.core.mail.backends.smtp.EmailBackend";
const CONSOLE_BACKEND: &str = "django.core.mail.backends.console.EmailBackend";
const LOCMEM_BACKEND: &str = "django.core.mail.backends.locmem.EmailBackend";

/// Boot-time settings for the Rust server and worker.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub profile: Profile,
    pub secret_key: String,
    /// True when no `SECRET_KEY` was set and one was generated (sessions do
    /// not survive restarts then — same as Python's random fallback).
    pub secret_key_generated: bool,
    pub debug: bool,
    pub allowed_hosts: Vec<String>,
    pub gitlab_allowed_hosts: Vec<String>,
    pub cors_allowed_origins: Vec<String>,
    pub cors_allow_all_origins: bool,
    pub database: DatabaseSettings,
    pub redis: RedisSettings,
    pub assistant: AssistantSettings,
    pub loop_tuning: LoopSettings,
    pub storage: StorageSettings,
    pub rabbitmq: RabbitSettings,
    pub runner: RunnerSettings,
    pub managed_runner: ManagedRunnerSettings,
    pub cloud_agent: CloudAgentSettings,
    pub default_agent_executor: String,
    pub session: SessionSettings,
    pub urls: UrlSettings,
    pub email_backend: String,
    pub file_size_limit: i64,
    pub github_sync_enabled: bool,
    pub scheduler_enabled: bool,
    pub scout_monitor: bool,
    pub scout_key: String,
    pub posthog_api_key: Option<String>,
    pub posthog_host: Option<String>,
    pub hard_delete_after_days: i64,
    pub instance_changelog_url: String,
    pub enable_drf_spectacular: bool,
    /// `production.py` only: honor `X-Forwarded-Proto` for `is_secure()`.
    pub secure_proxy_ssl_header: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DatabaseSettings {
    pub url: Option<String>,
    pub name: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub host: Option<String>,
    pub port: String,
    pub read_replica: Option<ReplicaSettings>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReplicaSettings {
    pub url: Option<String>,
    pub name: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub host: Option<String>,
    pub port: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RedisSettings {
    pub url: Option<String>,
    pub ssl: bool,
    pub socket_connect_timeout_secs: f64,
    pub socket_timeout_secs: f64,
    pub health_check_interval_secs: i64,
    pub max_connections: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssistantSettings {
    pub crypto_backend: String,
    pub kms_key_id: String,
    pub kms_endpoint_url: String,
    pub encryption_key: String,
    pub key_cache_ttl_secs: i64,
    pub key_cache_maxsize: i64,
    pub block_private_urls: bool,
    pub turn_soft_limit: i64,
    pub turn_hard_limit: i64,
    pub history_max_turns: i64,
    pub loop_history_max_turns: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoopSettings {
    pub enabled: bool,
    pub stagger_window_minutes: i64,
    pub max_dispatch_per_tick: i64,
    pub reconcile_every_minutes: i64,
    pub rotation_headroom: i64,
    pub max_writes: i64,
    pub pr_lookups_per_run: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StorageSettings {
    pub use_minio: bool,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub bucket_name: String,
    pub region: String,
    pub endpoint_url: Option<String>,
    pub signed_url_expiration_secs: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RabbitSettings {
    pub host: String,
    pub port: String,
    pub user: String,
    pub password: String,
    pub vhost: String,
    pub amqp_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunnerSettings {
    pub long_poll_interval_secs: i64,
    pub access_token_ttl_secs: i64,
    pub offline_threshold_secs: i64,
    pub offline_stream_ttl_secs: i64,
    pub offline_stream_maxlen: i64,
    pub stream_min_retention_secs: i64,
    pub event_batch_max_age_ms: i64,
    pub event_batch_max_bytes: i64,
    pub run_message_dedupe_ttl_secs: i64,
    pub latest_runner_version: Option<String>,
    pub min_runner_version: Option<String>,
    pub agent_stall_threshold_secs: i64,
    pub agent_observability_stale_secs: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManagedRunnerSettings {
    pub enabled: bool,
    pub max_per_user_project: i64,
    pub queued_max_age_secs: i64,
    pub graceful_stop_secs: i64,
    pub sweep_interval_secs: i64,
    pub desktop_min_version: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CloudAgentSettings {
    pub enabled: bool,
    pub writes_enabled: bool,
    pub github_tools_enabled: bool,
    pub disabled_tools: Vec<String>,
    pub reconcile_interval_secs: i64,
    pub model_request_timeout_secs: i64,
    pub execution_timeout_secs: i64,
    pub run_soft_limit_secs: i64,
    pub run_hard_limit_secs: i64,
    pub stale_grace_secs: i64,
    pub dispatch_lease_secs: i64,
    pub dispatch_backoff_secs: i64,
    pub dispatch_scan_interval_secs: i64,
    pub sweep_interval_secs: i64,
    pub dispatch_scan_batch: i64,
    pub max_queue_age_secs: i64,
    pub model_request_limit: i64,
    pub tool_call_limit: i64,
    pub write_call_limit: i64,
    pub input_token_limit: i64,
    pub output_token_limit: i64,
    pub total_token_limit: i64,
    pub max_output_tokens_per_request: i64,
    pub max_queued_per_workspace: i64,
    pub max_running_per_workspace: i64,
    pub user_creation_rate_per_minute: i64,
    pub workspace_creation_rate_per_minute: i64,
    pub tool_timeout_secs: i64,
    pub max_tool_result_bytes: i64,
    pub max_prompt_bytes: i64,
    pub max_final_result_bytes: i64,
    pub max_events: i64,
    pub block_private_urls: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionSettings {
    pub cookie_age_secs: i64,
    pub cookie_name: String,
    pub cookie_domain: Option<String>,
    /// `SESSION_COOKIE_SECURE = secure_origins`: true when explicit origins
    /// exist and none is plain `http:`; false with no origins set.
    pub cookie_secure: bool,
    pub save_every_request: bool,
    pub admin_cookie_age_secs: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UrlSettings {
    pub admin_base_url: Option<String>,
    pub admin_base_path: String,
    pub space_base_url: Option<String>,
    pub space_base_path: String,
    pub app_base_url: Option<String>,
    pub app_base_path: String,
    pub live_base_url: Option<String>,
    pub live_base_path: String,
    pub web_url: Option<String>,
}

/// Resolution context: registry + variable map. All reads flow through the
/// accessor, so the tier rule and defaults apply uniformly.
struct Ctx<'a> {
    registry: &'a ConfigRegistry,
    vars: &'a HashMap<String, String>,
}

impl<'a> Ctx<'a> {
    fn lookup(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }

    fn raw(&self, key: &str, inline: Option<&ConfigValue>) -> Result<ConfigValue, ConfigError> {
        get_env_with(self.registry, &|k| self.lookup(k), key, inline)
    }

    fn req_string(&self, key: &str, inline: &str) -> Result<String, ConfigError> {
        let inline_v = ConfigValue::from(inline);
        match self.raw(key, Some(&inline_v))? {
            ConfigValue::Str(s) => Ok(s),
            other => Err(ConfigError::TypeMismatch {
                key: key.to_owned(),
                expected: "string",
                actual: format!("{other:?}"),
            }),
        }
    }

    fn opt_string(&self, key: &str) -> Result<Option<String>, ConfigError> {
        match self.raw(key, None)? {
            ConfigValue::Null | ConfigValue::Bool(false) => Ok(None),
            ConfigValue::Str(s) => Ok(Some(s)),
            other => Err(ConfigError::TypeMismatch {
                key: key.to_owned(),
                expected: "string or null",
                actual: format!("{other:?}"),
            }),
        }
    }

    fn req_int(&self, key: &str, inline: ConfigValue) -> Result<i64, ConfigError> {
        match self.raw(key, Some(&inline))?.to_int() {
            Some(i) => Ok(i),
            None => Err(ConfigError::TypeMismatch {
                key: key.to_owned(),
                expected: "int",
                actual: self.lookup(key).unwrap_or_default(),
            }),
        }
    }

    fn opt_int(&self, key: &str) -> Result<Option<i64>, ConfigError> {
        match self.raw(key, None)? {
            ConfigValue::Null => Ok(None),
            v => v
                .to_int()
                .map(Some)
                .ok_or_else(|| ConfigError::TypeMismatch {
                    key: key.to_owned(),
                    expected: "int or null",
                    actual: self.lookup(key).unwrap_or_default(),
                }),
        }
    }

    fn req_float(&self, key: &str, inline: ConfigValue) -> Result<f64, ConfigError> {
        match self.raw(key, Some(&inline))?.to_float() {
            Some(f) => Ok(f),
            None => Err(ConfigError::TypeMismatch {
                key: key.to_owned(),
                expected: "float",
                actual: self.lookup(key).unwrap_or_default(),
            }),
        }
    }

    /// `value == "1"` (case-sensitive, no lowering): session persistence,
    /// spectacular gate, read-replica switch.
    fn flag_1(&self, key: &str, inline: &str) -> Result<bool, ConfigError> {
        Ok(self.req_string(key, inline)? == "1")
    }

    /// `value.lower() in ("1", "true", "yes")`: the dominant convention.
    fn flag_tri(&self, key: &str, inline: &str) -> Result<bool, ConfigError> {
        Ok(matches!(
            self.req_string(key, inline)?.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        ))
    }

    /// `value.lower() == "true"`: the sync/scheduler toggles.
    fn flag_true(&self, key: &str, inline: &str) -> Result<bool, ConfigError> {
        Ok(self.req_string(key, inline)?.to_ascii_lowercase().as_str() == "true")
    }

    fn csv_stripped(&self, key: &str, inline: &str) -> Result<Vec<String>, ConfigError> {
        Ok(self
            .req_string(key, inline)?
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect())
    }
}

/// `is_valid_url` (`pi_dash/utils/url.py`): scheme and host both present.
/// This covers the base-URL guards; full URL parsing is a consumer concern.
fn is_valid_url(s: &str) -> bool {
    match s.split_once("://") {
        Some((scheme, rest)) => {
            let host = rest.split(['/', '?', '#']).next().unwrap_or("");
            !scheme.is_empty()
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
                && !host.is_empty()
        }
        None => false,
    }
}

fn validated_base_url(v: Option<String>) -> Option<String> {
    match v {
        // `if ADMIN_BASE_URL and not is_valid_url(...)`: empty stays as-is,
        // invalid non-empty becomes None.
        None => None,
        Some(s) if s.is_empty() || is_valid_url(&s) => Some(s),
        Some(_) => None,
    }
}

fn generate_secret_key() -> String {
    use rand::Rng as _;
    rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(64)
        .map(char::from)
        .collect()
}

impl Settings {
    /// Resolve from the process environment with the common profile and no
    /// overlay. Fails loudly on a `Db`-tier read or an unparsable value —
    /// the equivalent of the settings module raising at import.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_env_with(Profile::Common, &NoOverlay)
    }

    /// Resolve from the process environment with a profile and overlay.
    /// The binary's `main` (OSS or private) calls this before the app
    /// builder.
    pub fn from_env_with(
        profile: Profile,
        overlay: &dyn SettingsOverlay,
    ) -> Result<Self, ConfigError> {
        let vars: HashMap<String, String> = std::env::vars().collect();
        Self::from_map_with(&vars, profile, overlay)
    }

    /// Deterministic defaults without touching the process environment
    /// (empty map, common profile, no overlay). Used by `AppState::new` and
    /// unit tests; a generated `SECRET_KEY` is flagged.
    pub fn test_defaults() -> Self {
        Self::from_map_with(&HashMap::new(), Profile::Common, &NoOverlay)
            .expect("empty map resolves against registry defaults")
    }

    /// Resolve from an explicit variable map. The overlay's `env_keys`
    /// reclassify first, then `PIDASH_CONFIG_ENV_KEYS`... in that order of
    /// precedence reversed: the process env var applies first so an explicit
    /// programmatic overlay wins, mirroring Python's
    /// `{**_load_env_overrides(), **CONFIG_SOURCE_OVERRIDES}`.
    pub fn from_map_with(
        vars: &HashMap<String, String>,
        profile: Profile,
        overlay: &dyn SettingsOverlay,
    ) -> Result<Self, ConfigError> {
        let mut overrides: HashMap<String, ConfigSource> = vars
            .get(super::registry::ENV_KEYS_OVERRIDE_VAR)
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|k| !k.is_empty())
                    .map(|k| (k.to_owned(), ConfigSource::Env))
                    .collect()
            })
            .unwrap_or_default();
        for key in overlay.env_keys() {
            overrides.insert((*key).to_owned(), ConfigSource::Env);
        }
        let registry = ConfigRegistry::build_with_overrides(overrides);
        let ctx = Ctx {
            registry: &registry,
            vars,
        };
        let mut settings = Self::resolve(&ctx, profile)?;
        overlay.apply(&mut settings);
        Ok(settings)
    }

    #[allow(clippy::too_many_lines)]
    fn resolve(ctx: &Ctx<'_>, profile: Profile) -> Result<Self, ConfigError> {
        let false_v = ConfigValue::from(false);

        // SECRET_KEY: env or a generated fallback (flagged), like
        // get_random_secret_key() in common.py.
        let (secret_key, secret_key_generated) = match ctx.raw("SECRET_KEY", None)? {
            ConfigValue::Str(s) if !s.is_empty() => (s, false),
            _ => {
                tracing::warn!(
                    "SECRET_KEY unset; generated an ephemeral one (sessions will not survive restarts)"
                );
                (generate_secret_key(), true)
            }
        };

        // common.py: DEBUG = int(get_config("DEBUG", "0")) — garbage fails boot.
        let mut debug = ctx.req_int("DEBUG", ConfigValue::from("0"))? != 0;

        // PORT... (no PORT key; bind address is a CLI flag, not a setting.)

        let database_url = match ctx.opt_string("DATABASE_URL")? {
            Some(s) if s.is_empty() => None,
            v => v,
        };

        let enable_read_replica = ctx.flag_1("ENABLE_READ_REPLICA", "0")?;
        let read_replica = if enable_read_replica {
            Some(ReplicaSettings {
                url: ctx.opt_string("DATABASE_READ_REPLICA_URL")?,
                name: ctx.opt_string("POSTGRES_READ_REPLICA_DB")?,
                user: ctx.opt_string("POSTGRES_READ_REPLICA_USER")?,
                password: ctx.opt_string("POSTGRES_READ_REPLICA_PASSWORD")?,
                host: ctx.opt_string("POSTGRES_READ_REPLICA_HOST")?,
                port: ctx.req_string("POSTGRES_READ_REPLICA_PORT", "5432")?,
            })
        } else {
            None
        };

        let redis_url = ctx.opt_string("REDIS_URL")?;
        let redis_ssl = redis_url
            .as_deref()
            .map(|u| u.contains("rediss"))
            .unwrap_or(false);

        let s3_or_minio = match ctx.opt_string("AWS_S3_ENDPOINT_URL")? {
            Some(s) if !s.is_empty() => Some(s),
            _ => match ctx.opt_string("MINIO_ENDPOINT_URL")? {
                Some(s) if !s.is_empty() => Some(s),
                _ => None,
            },
        };

        let mut email_backend = SMTP_BACKEND.to_owned();
        let mut secure_proxy_ssl_header = false;
        let mut web_url = ctx.opt_string("WEB_URL")?;
        let mut app_base_url = validated_base_url(ctx.opt_string("APP_BASE_URL")?);

        let cors_allowed_origins = ctx.csv_stripped("CORS_ALLOWED_ORIGINS", "")?;
        // secure_origins, ported exactly (substring match on "http:").
        let cookie_secure = !cors_allowed_origins.is_empty()
            && !cors_allowed_origins.iter().any(|o| o.contains("http:"));

        match profile {
            Profile::Common => {}
            Profile::Local => {
                debug = true;
                // local.py: get_config("EMAIL_BACKEND", <console>).
                email_backend = ctx.req_string("EMAIL_BACKEND", CONSOLE_BACKEND)?;
            }
            Profile::Production => {
                // production.py: DEBUG = int(get_config("DEBUG", 0)) == 1.
                // Garbage fails boot (Python raises out of int() there).
                debug = match ctx.raw("DEBUG", Some(&ConfigValue::from(0)))?.to_int() {
                    Some(i) => i == 1,
                    None => {
                        return Err(ConfigError::TypeMismatch {
                            key: "DEBUG".to_owned(),
                            expected: "int",
                            actual: ctx.lookup("DEBUG").unwrap_or_default(),
                        });
                    }
                };
                secure_proxy_ssl_header = true;
            }
            Profile::Test => {
                debug = true;
                email_backend = LOCMEM_BACKEND.to_owned();
                // test.py setdefault seeds for the harness.
                if web_url.is_none() {
                    web_url = Some("http://localhost".to_owned());
                }
                if app_base_url.is_none() {
                    app_base_url = Some("http://localhost".to_owned());
                }
            }
        }

        Ok(Self {
            profile,
            secret_key,
            secret_key_generated,
            debug,
            // Ported quirk: common.py splits without stripping, so
            // "a, b" yields ["a", " b"]. Kept byte-for-byte.
            allowed_hosts: ctx
                .req_string("ALLOWED_HOSTS", "*")?
                .split(',')
                .map(str::to_owned)
                .collect(),
            gitlab_allowed_hosts: ctx.csv_stripped("GITLAB_ALLOWED_HOSTS", "")?,
            cors_allowed_origins: cors_allowed_origins.clone(),
            cors_allow_all_origins: cors_allowed_origins.is_empty(),
            database: DatabaseSettings {
                url: database_url,
                name: ctx.opt_string("POSTGRES_DB")?,
                user: ctx.opt_string("POSTGRES_USER")?,
                password: ctx.opt_string("POSTGRES_PASSWORD")?,
                host: ctx.opt_string("POSTGRES_HOST")?,
                port: ctx.req_string("POSTGRES_PORT", "5432")?,
                read_replica,
            },
            redis: RedisSettings {
                url: redis_url,
                ssl: redis_ssl,
                socket_connect_timeout_secs: ctx
                    .req_float("REDIS_SOCKET_CONNECT_TIMEOUT", ConfigValue::from(2.0))?,
                socket_timeout_secs: ctx
                    .req_float("REDIS_SOCKET_TIMEOUT", ConfigValue::from(5.0))?,
                health_check_interval_secs: ctx
                    .req_int("REDIS_HEALTH_CHECK_INTERVAL", ConfigValue::from(30))?,
                max_connections: ctx.opt_int("REDIS_MAX_CONNECTIONS")?,
            },
            assistant: AssistantSettings {
                crypto_backend: ctx.req_string("ASSISTANT_CRYPTO_BACKEND", "aws-kms")?,
                kms_key_id: ctx.req_string("ASSISTANT_KMS_KEY_ID", "")?,
                kms_endpoint_url: ctx.req_string("ASSISTANT_KMS_ENDPOINT_URL", "")?,
                encryption_key: ctx.req_string("ASSISTANT_ENCRYPTION_KEY", "")?,
                key_cache_ttl_secs: ctx
                    .req_int("ASSISTANT_KEY_CACHE_TTL", ConfigValue::from(300))?,
                key_cache_maxsize: ctx
                    .req_int("ASSISTANT_KEY_CACHE_MAXSIZE", ConfigValue::from(1000))?,
                block_private_urls: ctx.flag_tri("ASSISTANT_BLOCK_PRIVATE_URLS", "false")?,
                turn_soft_limit: ctx
                    .req_int("ASSISTANT_TURN_SOFT_LIMIT", ConfigValue::from(300))?,
                turn_hard_limit: ctx
                    .req_int("ASSISTANT_TURN_HARD_LIMIT", ConfigValue::from(330))?,
                history_max_turns: ctx
                    .req_int("ASSISTANT_HISTORY_MAX_TURNS", ConfigValue::from(40))?,
                loop_history_max_turns: ctx
                    .req_int("ASSISTANT_LOOP_HISTORY_MAX_TURNS", ConfigValue::from(5))?,
            },
            loop_tuning: LoopSettings {
                enabled: ctx.flag_tri("LOOP_ENABLED", "true")?,
                stagger_window_minutes: ctx
                    .req_int("LOOP_STAGGER_WINDOW_MINUTES", ConfigValue::from(60))?,
                max_dispatch_per_tick: ctx
                    .req_int("LOOP_MAX_DISPATCH_PER_TICK", ConfigValue::from(100))?,
                reconcile_every_minutes: ctx
                    .req_int("LOOP_RECONCILE_EVERY_MINUTES", ConfigValue::from(15))?,
                rotation_headroom: ctx.req_int("LOOP_ROTATION_HEADROOM", ConfigValue::from(30))?,
                max_writes: ctx.req_int("LOOP_MAX_WRITES", ConfigValue::from(10))?,
                pr_lookups_per_run: ctx
                    .req_int("LOOP_PR_LOOKUPS_PER_RUN", ConfigValue::from(15))?,
            },
            storage: StorageSettings {
                use_minio: ctx.req_int("USE_MINIO", ConfigValue::from(0))? == 1,
                access_key_id: ctx.req_string("AWS_ACCESS_KEY_ID", "access-key")?,
                secret_access_key: ctx.req_string("AWS_SECRET_ACCESS_KEY", "secret-key")?,
                bucket_name: ctx.req_string("AWS_S3_BUCKET_NAME", "uploads")?,
                region: ctx.req_string("AWS_REGION", "")?,
                endpoint_url: s3_or_minio,
                signed_url_expiration_secs: ctx
                    .req_int("SIGNED_URL_EXPIRATION", ConfigValue::from("3600"))?,
            },
            rabbitmq: RabbitSettings {
                host: ctx.req_string("RABBITMQ_HOST", "localhost")?,
                port: ctx.req_string("RABBITMQ_PORT", "5672")?,
                user: ctx.req_string("RABBITMQ_USER", "guest")?,
                password: ctx.req_string("RABBITMQ_PASSWORD", "guest")?,
                vhost: ctx.req_string("RABBITMQ_VHOST", "/")?,
                amqp_url: ctx.opt_string("AMQP_URL")?,
            },
            runner: RunnerSettings {
                long_poll_interval_secs: {
                    let raw = ctx.req_int("LONG_POLL_INTERVAL_SECS", ConfigValue::from(25))?;
                    // common.py clamps to [1, 55] so the server-side block
                    // always finishes strictly before the daemon's
                    // per-request timeout, and warns so operators see the
                    // override at boot instead of silently getting another
                    // value.
                    if !(1..=55).contains(&raw) {
                        tracing::warn!(
                            "LONG_POLL_INTERVAL_SECS={raw} out of allowed range [1, 55]; \
                             clamping. Raising the upper bound requires also raising \
                             MAX_LONG_POLL_INTERVAL_SECS in runner/src/cloud/http.rs and the \
                             shared reqwest Client::timeout so daemon timeouts don't fire \
                             before the server's block_ms completes."
                        );
                    }
                    raw.clamp(1, 55)
                },
                access_token_ttl_secs: ctx
                    .req_int("ACCESS_TOKEN_TTL_SECS", ConfigValue::from(3600))?,
                offline_threshold_secs: ctx
                    .req_int("RUNNER_OFFLINE_THRESHOLD_SECS", ConfigValue::from(50))?,
                offline_stream_ttl_secs: ctx
                    .req_int("OFFLINE_STREAM_TTL_SECS", ConfigValue::from(86400))?,
                offline_stream_maxlen: ctx
                    .req_int("OFFLINE_STREAM_MAXLEN", ConfigValue::from(1000))?,
                stream_min_retention_secs: ctx
                    .req_int("RUNNER_STREAM_MIN_RETENTION_SECS", ConfigValue::from(3600))?,
                event_batch_max_age_ms: ctx
                    .req_int("EVENT_BATCH_MAX_AGE_MS", ConfigValue::from(250))?,
                event_batch_max_bytes: ctx
                    .req_int("EVENT_BATCH_MAX_BYTES", ConfigValue::from(65536))?,
                run_message_dedupe_ttl_secs: ctx
                    .req_int("RUN_MESSAGE_DEDUPE_TTL_SECS", ConfigValue::from(604800))?,
                latest_runner_version: ctx.opt_string("LATEST_RUNNER_VERSION")?,
                min_runner_version: ctx.opt_string("MIN_RUNNER_VERSION")?,
                agent_stall_threshold_secs: ctx
                    .req_int("RUNNER_AGENT_STALL_THRESHOLD_SECS", ConfigValue::from(360))?,
                agent_observability_stale_secs: ctx.req_int(
                    "RUNNER_AGENT_OBSERVABILITY_STALE_SECS",
                    ConfigValue::from(90),
                )?,
            },
            managed_runner: ManagedRunnerSettings {
                enabled: ctx.flag_tri("MANAGED_RUNNER_ENABLED", "false")?,
                max_per_user_project: ctx
                    .req_int("MANAGED_RUNNER_MAX_PER_USER_PROJECT", ConfigValue::from(1))?,
                queued_max_age_secs: ctx.req_int(
                    "MANAGED_RUNNER_QUEUED_MAX_AGE_SECS",
                    ConfigValue::from(43200),
                )?,
                graceful_stop_secs: ctx
                    .req_int("MANAGED_RUNNER_GRACEFUL_STOP_SECS", ConfigValue::from(30))?,
                sweep_interval_secs: ctx.req_int(
                    "MANAGED_RUNNER_SWEEP_INTERVAL_SECONDS",
                    ConfigValue::from(300),
                )?,
                desktop_min_version: ctx
                    .req_string("DESKTOP_MIN_VERSION_FOR_MANAGED_RUNNER", "")?,
            },
            cloud_agent: CloudAgentSettings {
                enabled: ctx.flag_tri("CLOUD_AGENT_ENABLED", "false")?,
                writes_enabled: ctx.flag_tri("CLOUD_AGENT_WRITES_ENABLED", "false")?,
                github_tools_enabled: ctx.flag_tri("CLOUD_AGENT_GITHUB_TOOLS_ENABLED", "true")?,
                disabled_tools: ctx.csv_stripped("CLOUD_AGENT_DISABLED_TOOLS", "")?,
                reconcile_interval_secs: ctx.req_int(
                    "AGENT_RUN_TERMINAL_RECONCILE_INTERVAL_SECONDS",
                    ConfigValue::from(30),
                )?,
                model_request_timeout_secs: ctx.req_int(
                    "CLOUD_AGENT_MODEL_REQUEST_TIMEOUT_SECONDS",
                    ConfigValue::from(60),
                )?,
                execution_timeout_secs: ctx.req_int(
                    "CLOUD_AGENT_EXECUTION_TIMEOUT_SECONDS",
                    ConfigValue::from(285),
                )?,
                run_soft_limit_secs: ctx
                    .req_int("CLOUD_AGENT_RUN_SOFT_LIMIT_SECONDS", ConfigValue::from(300))?,
                run_hard_limit_secs: ctx
                    .req_int("CLOUD_AGENT_RUN_HARD_LIMIT_SECONDS", ConfigValue::from(330))?,
                stale_grace_secs: ctx
                    .req_int("CLOUD_AGENT_STALE_GRACE_SECONDS", ConfigValue::from(60))?,
                dispatch_lease_secs: ctx
                    .req_int("CLOUD_AGENT_DISPATCH_LEASE_SECONDS", ConfigValue::from(60))?,
                dispatch_backoff_secs: ctx.req_int(
                    "CLOUD_AGENT_DISPATCH_BACKOFF_SECONDS",
                    ConfigValue::from(10),
                )?,
                dispatch_scan_interval_secs: ctx.req_int(
                    "CLOUD_AGENT_DISPATCH_SCAN_INTERVAL_SECONDS",
                    ConfigValue::from(10),
                )?,
                sweep_interval_secs: ctx
                    .req_int("CLOUD_AGENT_SWEEP_INTERVAL_SECONDS", ConfigValue::from(30))?,
                dispatch_scan_batch: ctx
                    .req_int("CLOUD_AGENT_DISPATCH_SCAN_BATCH", ConfigValue::from(100))?,
                max_queue_age_secs: ctx
                    .req_int("CLOUD_AGENT_MAX_QUEUE_AGE_SECONDS", ConfigValue::from(900))?,
                model_request_limit: ctx
                    .req_int("CLOUD_AGENT_MODEL_REQUEST_LIMIT", ConfigValue::from(25))?,
                tool_call_limit: ctx
                    .req_int("CLOUD_AGENT_TOOL_CALL_LIMIT", ConfigValue::from(20))?,
                write_call_limit: ctx
                    .req_int("CLOUD_AGENT_WRITE_CALL_LIMIT", ConfigValue::from(3))?,
                input_token_limit: ctx
                    .req_int("CLOUD_AGENT_INPUT_TOKEN_LIMIT", ConfigValue::from(144000))?,
                output_token_limit: ctx
                    .req_int("CLOUD_AGENT_OUTPUT_TOKEN_LIMIT", ConfigValue::from(16000))?,
                total_token_limit: ctx
                    .req_int("CLOUD_AGENT_TOTAL_TOKEN_LIMIT", ConfigValue::from(160000))?,
                max_output_tokens_per_request: ctx.req_int(
                    "CLOUD_AGENT_MAX_OUTPUT_TOKENS_PER_REQUEST",
                    ConfigValue::from(4096),
                )?,
                max_queued_per_workspace: ctx.req_int(
                    "CLOUD_AGENT_MAX_QUEUED_PER_WORKSPACE",
                    ConfigValue::from(20),
                )?,
                max_running_per_workspace: ctx.req_int(
                    "CLOUD_AGENT_MAX_RUNNING_PER_WORKSPACE",
                    ConfigValue::from(2),
                )?,
                user_creation_rate_per_minute: ctx.req_int(
                    "CLOUD_AGENT_USER_CREATION_RATE_PER_MINUTE",
                    ConfigValue::from(6),
                )?,
                workspace_creation_rate_per_minute: ctx.req_int(
                    "CLOUD_AGENT_WORKSPACE_CREATION_RATE_PER_MINUTE",
                    ConfigValue::from(30),
                )?,
                tool_timeout_secs: ctx
                    .req_int("CLOUD_AGENT_TOOL_TIMEOUT_SECONDS", ConfigValue::from(20))?,
                max_tool_result_bytes: ctx.req_int(
                    "CLOUD_AGENT_MAX_TOOL_RESULT_BYTES",
                    ConfigValue::from(65536),
                )?,
                max_prompt_bytes: ctx
                    .req_int("CLOUD_AGENT_MAX_PROMPT_BYTES", ConfigValue::from(262144))?,
                max_final_result_bytes: ctx.req_int(
                    "CLOUD_AGENT_MAX_FINAL_RESULT_BYTES",
                    ConfigValue::from(65536),
                )?,
                max_events: ctx.req_int("CLOUD_AGENT_MAX_EVENTS", ConfigValue::from(500))?,
                block_private_urls: ctx.flag_tri("CLOUD_AGENT_BLOCK_PRIVATE_URLS", "true")?,
            },
            default_agent_executor: ctx.req_string("DEFAULT_AGENT_EXECUTOR", "local_runner")?,
            session: SessionSettings {
                cookie_age_secs: ctx.req_int("SESSION_COOKIE_AGE", ConfigValue::from(604800))?,
                cookie_name: ctx.req_string("SESSION_COOKIE_NAME", "session-id")?,
                cookie_domain: ctx.opt_string("COOKIE_DOMAIN")?,
                cookie_secure,
                save_every_request: ctx.flag_1("SESSION_SAVE_EVERY_REQUEST", "0")?,
                admin_cookie_age_secs: ctx
                    .req_int("ADMIN_SESSION_COOKIE_AGE", ConfigValue::from(3600))?,
            },
            urls: UrlSettings {
                admin_base_url: validated_base_url(ctx.opt_string("ADMIN_BASE_URL")?),
                admin_base_path: ctx.req_string("ADMIN_BASE_PATH", "/god-mode/")?,
                space_base_url: validated_base_url(ctx.opt_string("SPACE_BASE_URL")?),
                space_base_path: ctx.req_string("SPACE_BASE_PATH", "/spaces/")?,
                app_base_url,
                app_base_path: ctx.req_string("APP_BASE_PATH", "/")?,
                live_base_url: validated_base_url(ctx.opt_string("LIVE_BASE_URL")?),
                live_base_path: ctx.req_string("LIVE_BASE_PATH", "/live/")?,
                web_url,
            },
            email_backend,
            file_size_limit: ctx.req_int("FILE_SIZE_LIMIT", ConfigValue::from(5242880))?,
            github_sync_enabled: ctx.flag_true("GITHUB_SYNC_ENABLED", "true")?,
            scheduler_enabled: ctx.flag_true("SCHEDULER_ENABLED", "true")?,
            scout_monitor: match ctx.raw("SCOUT_MONITOR", Some(&false_v))? {
                ConfigValue::Bool(b) => b,
                ConfigValue::Str(s) => {
                    matches!(s.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
                }
                ConfigValue::Int(i) => i != 0,
                ConfigValue::Float(f) => f != 0.0,
                ConfigValue::Null => false,
            },
            scout_key: ctx.req_string("SCOUT_KEY", "")?,
            posthog_api_key: ctx.opt_string("POSTHOG_API_KEY")?,
            posthog_host: ctx.opt_string("POSTHOG_HOST")?,
            hard_delete_after_days: ctx.req_int("HARD_DELETE_AFTER_DAYS", ConfigValue::from(60))?,
            instance_changelog_url: ctx.req_string("INSTANCE_CHANGELOG_URL", "")?,
            enable_drf_spectacular: ctx.flag_1("ENABLE_DRF_SPECTACULAR", "0")?,
            secure_proxy_ssl_header,
        })
    }

    /// Build the [`Keyring`] for decrypting db-tier secrets from these
    /// settings (the `SECRET_KEY` Python's `decrypt_data` closes over).
    pub fn keyring(&self) -> super::encryption::Keyring {
        super::encryption::Keyring::from_secret(&self.secret_key)
    }

    /// Every key `Settings` reads. The integrity test below asserts each is
    /// registered and `Env`-sourced — the Rust form of
    /// `test_registry_catalogs_every_key_settings_read` plus the boot-tier
    /// rule.
    #[cfg(test)]
    fn read_keys() -> &'static [&'static str] {
        &[
            "SECRET_KEY",
            "DEBUG",
            "ALLOWED_HOSTS",
            "GITLAB_ALLOWED_HOSTS",
            "CORS_ALLOWED_ORIGINS",
            "DATABASE_URL",
            "POSTGRES_DB",
            "POSTGRES_USER",
            "POSTGRES_PASSWORD",
            "POSTGRES_HOST",
            "POSTGRES_PORT",
            "ENABLE_READ_REPLICA",
            "DATABASE_READ_REPLICA_URL",
            "POSTGRES_READ_REPLICA_DB",
            "POSTGRES_READ_REPLICA_USER",
            "POSTGRES_READ_REPLICA_PASSWORD",
            "POSTGRES_READ_REPLICA_HOST",
            "POSTGRES_READ_REPLICA_PORT",
            "REDIS_URL",
            "REDIS_SOCKET_CONNECT_TIMEOUT",
            "REDIS_SOCKET_TIMEOUT",
            "REDIS_HEALTH_CHECK_INTERVAL",
            "REDIS_MAX_CONNECTIONS",
            "ASSISTANT_CRYPTO_BACKEND",
            "ASSISTANT_KMS_KEY_ID",
            "ASSISTANT_KMS_ENDPOINT_URL",
            "ASSISTANT_ENCRYPTION_KEY",
            "ASSISTANT_KEY_CACHE_TTL",
            "ASSISTANT_KEY_CACHE_MAXSIZE",
            "ASSISTANT_BLOCK_PRIVATE_URLS",
            "ASSISTANT_TURN_SOFT_LIMIT",
            "ASSISTANT_TURN_HARD_LIMIT",
            "ASSISTANT_HISTORY_MAX_TURNS",
            "ASSISTANT_LOOP_HISTORY_MAX_TURNS",
            "LOOP_ENABLED",
            "LOOP_STAGGER_WINDOW_MINUTES",
            "LOOP_MAX_DISPATCH_PER_TICK",
            "LOOP_RECONCILE_EVERY_MINUTES",
            "LOOP_ROTATION_HEADROOM",
            "LOOP_MAX_WRITES",
            "LOOP_PR_LOOKUPS_PER_RUN",
            "USE_MINIO",
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_S3_BUCKET_NAME",
            "AWS_REGION",
            "AWS_S3_ENDPOINT_URL",
            "MINIO_ENDPOINT_URL",
            "SIGNED_URL_EXPIRATION",
            "WEB_URL",
            "RABBITMQ_HOST",
            "RABBITMQ_PORT",
            "RABBITMQ_USER",
            "RABBITMQ_PASSWORD",
            "RABBITMQ_VHOST",
            "AMQP_URL",
            "LONG_POLL_INTERVAL_SECS",
            "ACCESS_TOKEN_TTL_SECS",
            "RUNNER_OFFLINE_THRESHOLD_SECS",
            "OFFLINE_STREAM_TTL_SECS",
            "OFFLINE_STREAM_MAXLEN",
            "RUNNER_STREAM_MIN_RETENTION_SECS",
            "EVENT_BATCH_MAX_AGE_MS",
            "EVENT_BATCH_MAX_BYTES",
            "RUN_MESSAGE_DEDUPE_TTL_SECS",
            "LATEST_RUNNER_VERSION",
            "MIN_RUNNER_VERSION",
            "RUNNER_AGENT_STALL_THRESHOLD_SECS",
            "RUNNER_AGENT_OBSERVABILITY_STALE_SECS",
            "MANAGED_RUNNER_ENABLED",
            "MANAGED_RUNNER_MAX_PER_USER_PROJECT",
            "MANAGED_RUNNER_QUEUED_MAX_AGE_SECS",
            "MANAGED_RUNNER_GRACEFUL_STOP_SECS",
            "MANAGED_RUNNER_SWEEP_INTERVAL_SECONDS",
            "DESKTOP_MIN_VERSION_FOR_MANAGED_RUNNER",
            "DEFAULT_AGENT_EXECUTOR",
            "AGENT_RUN_TERMINAL_RECONCILE_INTERVAL_SECONDS",
            "CLOUD_AGENT_ENABLED",
            "CLOUD_AGENT_WRITES_ENABLED",
            "CLOUD_AGENT_GITHUB_TOOLS_ENABLED",
            "CLOUD_AGENT_DISABLED_TOOLS",
            "CLOUD_AGENT_MODEL_REQUEST_TIMEOUT_SECONDS",
            "CLOUD_AGENT_EXECUTION_TIMEOUT_SECONDS",
            "CLOUD_AGENT_RUN_SOFT_LIMIT_SECONDS",
            "CLOUD_AGENT_RUN_HARD_LIMIT_SECONDS",
            "CLOUD_AGENT_STALE_GRACE_SECONDS",
            "CLOUD_AGENT_DISPATCH_LEASE_SECONDS",
            "CLOUD_AGENT_DISPATCH_BACKOFF_SECONDS",
            "CLOUD_AGENT_DISPATCH_SCAN_INTERVAL_SECONDS",
            "CLOUD_AGENT_SWEEP_INTERVAL_SECONDS",
            "CLOUD_AGENT_DISPATCH_SCAN_BATCH",
            "CLOUD_AGENT_MAX_QUEUE_AGE_SECONDS",
            "CLOUD_AGENT_MODEL_REQUEST_LIMIT",
            "CLOUD_AGENT_TOOL_CALL_LIMIT",
            "CLOUD_AGENT_WRITE_CALL_LIMIT",
            "CLOUD_AGENT_INPUT_TOKEN_LIMIT",
            "CLOUD_AGENT_OUTPUT_TOKEN_LIMIT",
            "CLOUD_AGENT_TOTAL_TOKEN_LIMIT",
            "CLOUD_AGENT_MAX_OUTPUT_TOKENS_PER_REQUEST",
            "CLOUD_AGENT_MAX_QUEUED_PER_WORKSPACE",
            "CLOUD_AGENT_MAX_RUNNING_PER_WORKSPACE",
            "CLOUD_AGENT_USER_CREATION_RATE_PER_MINUTE",
            "CLOUD_AGENT_WORKSPACE_CREATION_RATE_PER_MINUTE",
            "CLOUD_AGENT_TOOL_TIMEOUT_SECONDS",
            "CLOUD_AGENT_MAX_TOOL_RESULT_BYTES",
            "CLOUD_AGENT_MAX_PROMPT_BYTES",
            "CLOUD_AGENT_MAX_FINAL_RESULT_BYTES",
            "CLOUD_AGENT_MAX_EVENTS",
            "CLOUD_AGENT_BLOCK_PRIVATE_URLS",
            "SESSION_COOKIE_AGE",
            "SESSION_COOKIE_NAME",
            "COOKIE_DOMAIN",
            "SESSION_SAVE_EVERY_REQUEST",
            "ADMIN_SESSION_COOKIE_AGE",
            "ADMIN_BASE_URL",
            "ADMIN_BASE_PATH",
            "SPACE_BASE_URL",
            "SPACE_BASE_PATH",
            "APP_BASE_URL",
            "APP_BASE_PATH",
            "LIVE_BASE_URL",
            "LIVE_BASE_PATH",
            "EMAIL_BACKEND",
            "FILE_SIZE_LIMIT",
            "GITHUB_SYNC_ENABLED",
            "SCHEDULER_ENABLED",
            "SCOUT_MONITOR",
            "SCOUT_KEY",
            "POSTHOG_API_KEY",
            "POSTHOG_HOST",
            "HARD_DELETE_AFTER_DAYS",
            "INSTANCE_CHANGELOG_URL",
            "ENABLE_DRF_SPECTACULAR",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn empty_map_resolves_to_registry_defaults() {
        let s = Settings::from_map_with(&HashMap::new(), Profile::Common, &NoOverlay)
            .expect("defaults resolve");
        assert!(s.secret_key_generated);
        assert_eq!(s.secret_key.len(), 64);
        assert!(!s.debug);
        assert_eq!(s.allowed_hosts, vec!["*"]);
        assert!(s.cors_allow_all_origins);
        assert_eq!(s.database.url, None);
        assert_eq!(s.database.port, "5432");
        assert_eq!(s.database.read_replica, None);
        assert!(!s.redis.ssl);
        assert!(s.loop_tuning.enabled);
        assert_eq!(s.loop_tuning.max_writes, 10);
        assert_eq!(s.file_size_limit, 5242880);
        assert_eq!(s.email_backend, SMTP_BACKEND);
        // Non-Local profiles ignore an EMAIL_BACKEND env override, exactly
        // like common.py's hardcoded assignment.
        let s2 = Settings::from_map_with(
            &vars(&[("EMAIL_BACKEND", "custom")]),
            Profile::Common,
            &NoOverlay,
        )
        .expect("resolves");
        assert_eq!(s2.email_backend, SMTP_BACKEND);
    }

    #[test]
    fn env_overrides_defaults() {
        let s = Settings::from_map_with(
            &vars(&[
                ("SECRET_KEY", "s3cr3t"),
                ("DEBUG", "1"),
                ("ALLOWED_HOSTS", "a.example,b.example"),
                (
                    "CORS_ALLOWED_ORIGINS",
                    "https://a.example, https://b.example ",
                ),
                ("DATABASE_URL", "postgresql://db/pidash"),
                ("ENABLE_READ_REPLICA", "1"),
                ("POSTGRES_READ_REPLICA_HOST", "replica"),
                ("REDIS_URL", "rediss://cache:6379"),
                ("LOOP_ENABLED", "yes"),
                ("USE_MINIO", "1"),
                ("APP_BASE_URL", "not a url"),
                ("ADMIN_BASE_URL", "https://admin.example"),
            ]),
            Profile::Common,
            &NoOverlay,
        )
        .expect("resolves");
        assert_eq!(s.secret_key, "s3cr3t");
        assert!(!s.secret_key_generated);
        assert!(s.debug);
        assert_eq!(s.allowed_hosts, vec!["a.example", "b.example"]);
        assert!(!s.cors_allow_all_origins);
        assert_eq!(
            s.cors_allowed_origins,
            vec!["https://a.example", "https://b.example"]
        );
        assert_eq!(s.database.url.as_deref(), Some("postgresql://db/pidash"));
        assert!(s.redis.ssl);
        assert!(s.loop_tuning.enabled);
        assert!(s.storage.use_minio);
        assert!(s.database.read_replica.is_some());
        // Invalid base URLs degrade to None (is_valid_url guard).
        assert_eq!(s.urls.app_base_url, None);
        assert_eq!(
            s.urls.admin_base_url.as_deref(),
            Some("https://admin.example")
        );
    }

    #[test]
    fn profiles_apply_their_deltas() {
        let local = Settings::from_map_with(&HashMap::new(), Profile::Local, &NoOverlay)
            .expect("local resolves");
        assert!(local.debug);
        assert_eq!(local.email_backend, CONSOLE_BACKEND);

        let prod = Settings::from_map_with(&HashMap::new(), Profile::Production, &NoOverlay)
            .expect("prod resolves");
        assert!(!prod.debug);
        assert!(prod.secure_proxy_ssl_header);
        let prod_debug =
            Settings::from_map_with(&vars(&[("DEBUG", "1")]), Profile::Production, &NoOverlay)
                .expect("prod resolves");
        assert!(prod_debug.debug);

        let test = Settings::from_map_with(&HashMap::new(), Profile::Test, &NoOverlay)
            .expect("test resolves");
        assert!(test.debug);
        assert_eq!(test.email_backend, LOCMEM_BACKEND);
        assert_eq!(test.urls.web_url.as_deref(), Some("http://localhost"));
        assert_eq!(test.urls.app_base_url.as_deref(), Some("http://localhost"));
    }

    #[test]
    fn overlay_reclassifies_and_mutates() {
        struct Cloud;
        impl SettingsOverlay for Cloud {
            fn env_keys(&self) -> &[&str] {
                &["EMAIL_HOST"]
            }
            fn apply(&self, settings: &mut Settings) {
                settings.debug = true;
            }
        }
        // EMAIL_HOST is Db-tier: without the overlay this fails at boot.
        assert!(Settings::from_map_with(&HashMap::new(), Profile::Common, &NoOverlay).is_ok());
        let reg = ConfigRegistry::build_with_overrides(HashMap::new());
        assert_eq!(
            get_env_with(&reg, &|_| None, "EMAIL_HOST", None).unwrap_err(),
            ConfigError::DbAtBoot("EMAIL_HOST".to_owned())
        );
        // With the overlay it resolves from the map, and apply() ran.
        let s = Settings::from_map_with(
            &vars(&[("EMAIL_HOST", "smtp.cloud")]),
            Profile::Common,
            &Cloud,
        )
        .expect("overlay resolves");
        assert!(s.debug);
    }

    #[test]
    fn env_keys_var_reclassifies_without_code() {
        // The cloud's SSM-native seam: PIDASH_CONFIG_ENV_KEYS forces a
        // Db key to Env with no overlay struct involved.
        let reg_check = |vars: &HashMap<String, String>| {
            Settings::from_map_with(vars, Profile::Common, &NoOverlay).is_ok()
        };
        assert!(reg_check(&HashMap::new()));
        let with_key = vars(&[
            ("PIDASH_CONFIG_ENV_KEYS", "EMAIL_HOST"),
            ("EMAIL_HOST", "smtp.ssm"),
        ]);
        assert!(reg_check(&with_key));
    }

    #[test]
    fn garbage_int_fails_boot_loudly() {
        let err = Settings::from_map_with(
            &vars(&[("FILE_SIZE_LIMIT", "huge")]),
            Profile::Common,
            &NoOverlay,
        )
        .expect_err("garbage int must fail boot");
        assert!(
            matches!(err, ConfigError::TypeMismatch { .. }),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn long_poll_interval_clamps_to_1_55() {
        // common.py clamps LONG_POLL_INTERVAL_SECS to [1, 55] so the
        // server-side block finishes before the daemon's per-request timeout.
        let s = Settings::from_map_with(&HashMap::new(), Profile::Common, &NoOverlay)
            .expect("resolves");
        assert_eq!(s.runner.long_poll_interval_secs, 25);
        for (raw, clamped) in [("0", 1), ("1", 1), ("30", 30), ("55", 55), ("600", 55)] {
            let s = Settings::from_map_with(
                &vars(&[("LONG_POLL_INTERVAL_SECS", raw)]),
                Profile::Common,
                &NoOverlay,
            )
            .expect("resolves");
            assert_eq!(s.runner.long_poll_interval_secs, clamped, "raw={raw}");
        }
    }

    #[test]
    fn every_read_key_is_registered_env_tier() {
        let reg = ConfigRegistry::build_with_overrides(HashMap::new());
        for key in Settings::read_keys() {
            let entry = reg
                .get(key)
                .unwrap_or_else(|| panic!("{key} not registered"));
            assert_eq!(
                entry.source,
                ConfigSource::Env,
                "{key} must be env-tier for boot"
            );
        }
    }

    #[test]
    fn settings_keyring_round_trips() {
        let s = Settings::from_map_with(
            &vars(&[("SECRET_KEY", "test-secret-key")]),
            Profile::Common,
            &NoOverlay,
        )
        .expect("resolves");
        let token = s.keyring().encrypt("s3cret");
        assert_eq!(s.keyring().decrypt(&token), "s3cret");
    }

    #[test]
    fn cookie_secure_matches_secure_origins_rule() {
        // No origins -> False.
        let s = Settings::from_map_with(&HashMap::new(), Profile::Common, &NoOverlay)
            .expect("resolves");
        assert!(!s.session.cookie_secure);
        // https origins do not contain the "http:" substring -> True.
        let s = Settings::from_map_with(
            &vars(&[("CORS_ALLOWED_ORIGINS", "https://a.example")]),
            Profile::Common,
            &NoOverlay,
        )
        .expect("resolves");
        assert!(s.session.cookie_secure);
        // http origins contain it -> False.
        let s = Settings::from_map_with(
            &vars(&[("CORS_ALLOWED_ORIGINS", "http://a.example")]),
            Profile::Common,
            &NoOverlay,
        )
        .expect("resolves");
        assert!(!s.session.cookie_secure);
        // Origins without the substring -> True.
        let s = Settings::from_map_with(
            &vars(&[("CORS_ALLOWED_ORIGINS", "localhost:3000")]),
            Profile::Common,
            &NoOverlay,
        )
        .expect("resolves");
        assert!(s.session.cookie_secure);
    }

    #[test]
    fn allowed_hosts_split_has_no_strip() {
        // Ported quirk: "a, b" keeps the space, like Python's split(",").
        let s = Settings::from_map_with(
            &vars(&[("ALLOWED_HOSTS", "a.example, b.example")]),
            Profile::Common,
            &NoOverlay,
        )
        .expect("resolves");
        assert_eq!(s.allowed_hosts, vec!["a.example", " b.example"]);
    }
}
