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
    /// `MINIO_ENDPOINT_SSL == "1"`: sign `https://{Host}` in MinIO mode
    /// (`storage.py:46-51`); otherwise the request scheme. Deployments
    /// ship `0`; only TLS-MinIO operators set it.
    pub minio_endpoint_ssl: bool,
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

    /// Shared-config-file region: the tail of botocore's default-region
    /// chain (`configprovider.py:56` + `session.py:full_config`), consulted
    /// only when neither `AWS_REGION` nor `AWS_DEFAULT_REGION` is set.
    /// Profile is `AWS_DEFAULT_PROFILE`, else `AWS_PROFILE`, else
    /// `default`; paths are `AWS_CONFIG_FILE` /
    /// `AWS_SHARED_CREDENTIALS_FILE`, else `$HOME/.aws/config` /
    /// `$HOME/.aws/credentials` — all direct map lookups, never
    /// `std::env`, so explicit maps (and tests) stay hermetic. A set-but
    /// missing profile fails (`ProfileNotFound`); a malformed file fails
    /// (`ConfigParseError`); both escape boto3 client creation in Django
    /// and fail the boot here instead. `$VAR` expansion in the paths,
    /// `~user` homes, and the `pwd` fallback when `HOME` is unset are
    /// not ported (documented deviations).
    fn shared_config_region(&self) -> Result<Option<String>, ConfigError> {
        let (profile_var, profile) = match self.lookup("AWS_DEFAULT_PROFILE") {
            some @ Some(_) => ("AWS_DEFAULT_PROFILE", some),
            None => ("AWS_PROFILE", self.lookup("AWS_PROFILE")),
        };
        let home = self.lookup("HOME");
        let config_path = self
            .lookup("AWS_CONFIG_FILE")
            .map(|p| expand_home_prefix(&p, home.as_deref()))
            .or_else(|| home.clone().map(|h| format!("{h}/.aws/config")));
        let creds_path = self
            .lookup("AWS_SHARED_CREDENTIALS_FILE")
            .map(|p| expand_home_prefix(&p, home.as_deref()))
            .or_else(|| home.map(|h| format!("{h}/.aws/credentials")));
        let profiles = load_shared_profiles(config_path.as_deref(), creds_path.as_deref())?;
        match profile {
            None => Ok(profiles
                .get("default")
                .and_then(|p| p.get("region"))
                .cloned()),
            Some(name) => match profiles.get(&name) {
                Some(scoped) => Ok(scoped.get("region").cloned()),
                None => Err(ConfigError::TypeMismatch {
                    key: profile_var.to_owned(),
                    expected: "a profile present in the AWS shared config",
                    actual: name,
                }),
            },
        }
    }

    /// S3 scope region (`S3Storage.__init__`, `storage.py:40,64`): Django
    /// passes `get_config("AWS_REGION", None)` to boto3, so unset is None
    /// (botocore's default-region chain) while set — even empty — is
    /// verbatim (botocore signs an empty scope part against a custom
    /// endpoint, and raises `ValueError` when deriving one). The inline
    /// Null default preserves the distinction: unset resolves to it, set
    /// resolves to the value. Unset then follows botocore's chain —
    /// `AWS_DEFAULT_REGION` verbatim (even `""`), else the shared-config
    /// tail above, else the S3 partition default `us-east-1`. botocore
    /// reads `AWS_DEFAULT_REGION` straight from the environment rather
    /// than through `get_config`, so this is a direct map lookup, not a
    /// registry read (and stays out of `read_keys`).
    fn s3_scope_region(&self) -> Result<String, ConfigError> {
        match self.raw("AWS_REGION", Some(&ConfigValue::Null))? {
            ConfigValue::Null => {
                if let Some(region) = self.lookup("AWS_DEFAULT_REGION") {
                    return Ok(region);
                }
                if let Some(region) = self.shared_config_region()? {
                    return Ok(region);
                }
                Ok("us-east-1".to_owned())
            }
            ConfigValue::Str(s) => Ok(s),
            other => Err(ConfigError::TypeMismatch {
                key: "AWS_REGION".to_owned(),
                expected: "string or null",
                actual: format!("{other:?}"),
            }),
        }
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

/// `validate_region_name` (`botocore/utils.py:1311`, called unconditionally
/// from `session.py:1014` during client creation): the resolved region
/// must be a valid host label —
/// `^(?![0-9]+$)(?!-)[a-zA-Z0-9-]{,63}(?<!-)$` — else `InvalidRegionError`.
/// The empty name matches (the class repeats zero times), so `""` passes
/// here and fails later at endpoint derivation (`ValueError`) when no
/// endpoint URL is configured. Manual port: the `regex` crate has no
/// lookaround.
pub fn is_valid_region_name(region: &str) -> bool {
    if region.len() > 63
        || !region
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return false;
    }
    if region.starts_with('-') || region.ends_with('-') {
        return false;
    }
    // `(?![0-9]+$)`: all-digit names are rejected, but the empty name
    // has no digits to match.
    region.is_empty() || !region.bytes().all(|b| b.is_ascii_digit())
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

/// Close an open INI pair: a multi-line `region` value fails (botocore
/// parses it as a nested dict and dies downstream — `ConfigParseError`
/// for `k = v`-less lines, `TypeError` past `validate_region_name`
/// otherwise); other keys' continuations must be well-formed nested
/// `k = v` lines or botocore's `_parse_nested` fails there instead —
/// but only when the base value is empty (the `startswith('\n')` gate
/// in `raw_config_parse`): continuations under a non-empty value stay
/// a plain string and never fail. Either way the failure is loud on
/// both sides.
fn close_ini_key(key: &str, base_empty: bool, conts: &[String]) -> Result<(), String> {
    if conts.is_empty() {
        return Ok(());
    }
    if key == "region" {
        return Err("multi-line value for \"region\"".to_owned());
    }
    if !base_empty {
        return Ok(());
    }
    for line in conts {
        if !line.contains('=') {
            return Err(format!("malformed nested line under {key:?}"));
        }
    }
    Ok(())
}

/// Parsed shared-config INI: section names in file order, each with
/// its lowercased keys in file order.
type IniSections = Vec<(String, Vec<(String, String)>)>;

/// Leading-`~` expansion for explicit shared-config paths
/// (`os.path.expanduser`, minus `~user` and the `pwd` fallback): `~/...`
/// and bare `~` resolve against the map's `HOME`; anything else —
/// including `~user/...` and an unset `HOME` — passes through verbatim
/// (and then misses the filesystem, like a missing file).
fn expand_home_prefix(path: &str, home: Option<&str>) -> String {
    let Some(home) = home else {
        return path.to_owned();
    };
    if path == "~" {
        return home.to_owned();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return format!("{home}/{rest}");
    }
    path.to_owned()
}

/// AWS shared-config INI (`botocore/configloader.py:raw_config_parse`):
/// `[section]` headers, `key = value` / `key: value` pairs, `#`/`;`
/// full-line comments (indented or not), blank lines. Keys are lowercased
/// (`optionxform`); section names are case-sensitive and untrimmed;
/// trailing text after a header's closing bracket is ignored. Returns
/// sections in file order. Fails exactly where botocore raises
/// `ConfigParseError`: duplicate sections/keys, garbage lines, keys
/// before any section, and indented lines with no key to continue.
/// A repeated `[DEFAULT]` reopens the defaults instead of failing.
fn parse_shared_ini(text: &str) -> Result<IniSections, String> {
    let mut sections: IniSections = Vec::new();
    let mut current: Option<usize> = None;
    let mut open_key: Option<(String, bool)> = None;
    let mut conts: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with('#') || stripped.starts_with(';') {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            match open_key {
                Some(_) => conts.push(stripped.to_owned()),
                None => return Err("indented line with no key to continue".to_owned()),
            }
            continue;
        }
        if let Some(rest) = stripped.strip_prefix('[') {
            if let Some((key, base_empty)) = open_key.take() {
                close_ini_key(&key, base_empty, &conts)?;
                conts.clear();
            }
            // `SECTCRE` is greedy: the header runs to the LAST bracket.
            let Some(end) = rest.rfind(']') else {
                return Err(format!("unclosed section header {stripped:?}"));
            };
            let name = rest[..end].to_owned();
            // A repeated `[DEFAULT]` reopens the defaults (configparser
            // exempts it from the duplicate-section error and merges);
            // routing to the existing entry keeps the duplicate-key
            // check across repeats too.
            if name == "DEFAULT" {
                if let Some(idx) = sections.iter().position(|(n, _)| n == "DEFAULT") {
                    current = Some(idx);
                    continue;
                }
            }
            if sections.iter().any(|(n, _)| n == &name) {
                return Err(format!("duplicate section {name:?}"));
            }
            sections.push((name, Vec::new()));
            current = Some(sections.len() - 1);
            continue;
        }
        let Some(sec) = current else {
            return Err(format!("keys before any section: {stripped:?}"));
        };
        let cut = stripped.find(['=', ':']);
        let Some(cut) = cut else {
            return Err(format!("malformed line {stripped:?}"));
        };
        let key = stripped[..cut].trim().to_ascii_lowercase();
        if key.is_empty() {
            return Err(format!("malformed line {stripped:?}"));
        }
        if let Some((open, base_empty)) = open_key.take() {
            close_ini_key(&open, base_empty, &conts)?;
            conts.clear();
        }
        if sections[sec].1.iter().any(|(k, _)| k == &key) {
            return Err(format!("duplicate key {key:?}"));
        }
        let value = stripped[cut + 1..].trim().to_owned();
        let base_empty = value.is_empty();
        sections[sec].1.push((key.clone(), value));
        open_key = Some((key, base_empty));
    }
    if let Some((key, base_empty)) = open_key.take() {
        close_ini_key(&key, base_empty, &conts)?;
    }
    Ok(sections)
}

/// Minimal `shlex.split` for profile headers (`configloader.py:203`):
/// whitespace-separated tokens honoring single/double quotes. Backslash
/// escapes are literal here (botocore processes them; a profile name
/// with a backslash is not a real input). `None` on unbalanced quotes,
/// which botocore also drops.
fn split_profile_name(name: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;
    for c in name.chars() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                current.push(c);
            }
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
            in_token = true;
        } else if c.is_whitespace() {
            if in_token {
                tokens.push(std::mem::take(&mut current));
                in_token = false;
            }
        } else {
            current.push(c);
            in_token = true;
        }
    }
    if quote.is_some() {
        return None;
    }
    if in_token {
        tokens.push(current);
    }
    Some(tokens)
}

/// `[DEFAULT]` keys of one parsed file: configparser falls back to
/// them for every section (`cp.get`), so each profile starts from a
/// copy with its own keys overlaid. The section itself is never a
/// profile (`cp.sections()` excludes it).
fn ini_defaults(sections: &IniSections) -> HashMap<String, String> {
    sections
        .iter()
        .find(|(name, _)| name == "DEFAULT")
        .map(|(_, keys)| keys.iter().cloned().collect())
        .unwrap_or_default()
}

/// Profile map over one parsed config file
/// (`configloader.py:build_profile_map`): `[default]` is the `default`
/// profile; `[profile NAME]` (exactly two whitespace/quote-separated
/// tokens — `shlex.split`, so `[profilefoo bar]` maps `bar`) is `NAME`;
/// anything else is ignored. Later sections overwrite earlier ones.
/// Every profile inherits `[DEFAULT]` (explicit keys win).
fn config_profile_map(sections: &IniSections) -> HashMap<String, HashMap<String, String>> {
    let mut profiles: HashMap<String, HashMap<String, String>> = HashMap::new();
    let defaults = ini_defaults(sections);
    for (name, keys) in sections {
        let profile = if name == "default" {
            Some("default".to_owned())
        } else if name.starts_with("profile") {
            match split_profile_name(name) {
                Some(tokens) if tokens.len() == 2 => Some(tokens[1].clone()),
                _ => None,
            }
        } else {
            None
        };
        if let Some(profile) = profile {
            let mut merged = defaults.clone();
            merged.extend(keys.iter().cloned());
            profiles.insert(profile, merged);
        }
    }
    profiles
}

/// Merged profile map (`session.py:full_config`): config-file profiles
/// plus the credentials file's raw sections (bare `[name]` headers, no
/// `profile` prefix), credentials winning per key. A missing file is an
/// empty map (`ConfigNotFound` is swallowed); a present-but-unreadable
/// or malformed file fails (botocore lets `OSError`/`ConfigParseError`
/// escape client creation; the boot fails here instead).
fn load_shared_profiles(
    config_path: Option<&str>,
    creds_path: Option<&str>,
) -> Result<HashMap<String, HashMap<String, String>>, ConfigError> {
    fn load_one(path: &str) -> Result<Option<IniSections>, ConfigError> {
        if !std::path::Path::new(path).is_file() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(path).map_err(|e| ConfigError::TypeMismatch {
            key: path.to_owned(),
            expected: "a readable AWS shared-config file",
            actual: e.to_string(),
        })?;
        parse_shared_ini(&text)
            .map(Some)
            .map_err(|detail| ConfigError::TypeMismatch {
                key: path.to_owned(),
                expected: "valid AWS shared-config ini",
                actual: detail,
            })
    }
    let mut profiles = match config_path {
        Some(path) => match load_one(path)? {
            Some(sections) => config_profile_map(&sections),
            None => HashMap::new(),
        },
        None => HashMap::new(),
    };
    if let Some(path) = creds_path {
        if let Some(sections) = load_one(path)? {
            let defaults = ini_defaults(&sections);
            for (name, keys) in &sections {
                if name == "DEFAULT" {
                    continue;
                }
                let mut merged = defaults.clone();
                merged.extend(keys.iter().cloned());
                profiles.entry(name.clone()).or_default().extend(merged);
            }
        }
    }
    Ok(profiles)
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

impl StorageSettings {
    /// `S3Storage.__init__` `endpoint_protocol` (`storage.py:46-51`):
    /// `MINIO_ENDPOINT_SSL=1` signs `https://{Host}` in MinIO mode,
    /// otherwise the request scheme (`X-Forwarded-Proto` here, Django's
    /// `request.scheme` there). Gated on MinIO mode like the Python
    /// nesting; non-MinIO branches ignore the scheme either way.
    pub fn endpoint_protocol<'a>(&self, request_scheme: &'a str) -> &'a str {
        if self.use_minio && self.minio_endpoint_ssl {
            "https"
        } else {
            request_scheme
        }
    }
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
                // `storage.py:48`: `get_config("MINIO_ENDPOINT_SSL", None) == "1"`.
                minio_endpoint_ssl: ctx.flag_1("MINIO_ENDPOINT_SSL", "")?,
                access_key_id: ctx.req_string("AWS_ACCESS_KEY_ID", "access-key")?,
                secret_access_key: ctx.req_string("AWS_SECRET_ACCESS_KEY", "secret-key")?,
                bucket_name: ctx.req_string("AWS_S3_BUCKET_NAME", "uploads")?,
                region: ctx.s3_scope_region()?,
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
            "MINIO_ENDPOINT_SSL",
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

    #[test]
    fn s3_scope_region_preserves_unset_vs_empty() {
        // Django passes get_config("AWS_REGION", None) to boto3: unset is
        // None (botocore chain -> us-east-1, probe A/E), set is verbatim —
        // even "" (probe D2/F). AWS_DEFAULT_REGION is the chain's middle
        // link, also verbatim (probe B/C2); explicit AWS_REGION wins (by
        // construction: Django passes it, botocore never consults the env).
        let unset = Settings::from_map_with(&HashMap::new(), Profile::Common, &NoOverlay)
            .expect("resolves");
        assert_eq!(unset.storage.region, "us-east-1");
        for (pairs, want) in [
            (vars(&[("AWS_DEFAULT_REGION", "eu-west-1")]), "eu-west-1"),
            (vars(&[("AWS_DEFAULT_REGION", "")]), ""),
            (vars(&[("AWS_REGION", "")]), ""),
            (vars(&[("AWS_REGION", "eu-central-1")]), "eu-central-1"),
            (
                vars(&[
                    ("AWS_REGION", "ap-south-1"),
                    ("AWS_DEFAULT_REGION", "eu-west-1"),
                ]),
                "ap-south-1",
            ),
        ] {
            let s = Settings::from_map_with(&pairs, Profile::Common, &NoOverlay).expect("resolves");
            assert_eq!(s.storage.region, want);
        }
    }

    #[test]
    fn minio_endpoint_ssl_is_exact_1() {
        // `storage.py:48`: only the exact string "1" forces https.
        let unset = Settings::from_map_with(&HashMap::new(), Profile::Common, &NoOverlay)
            .expect("resolves");
        assert!(!unset.storage.minio_endpoint_ssl);
        for (value, want) in [
            ("1", true),
            ("0", false),
            ("", false),
            ("true", false),
            ("https", false),
            ("01", false),
        ] {
            let s = Settings::from_map_with(
                &vars(&[("MINIO_ENDPOINT_SSL", value)]),
                Profile::Common,
                &NoOverlay,
            )
            .expect("resolves");
            assert_eq!(s.storage.minio_endpoint_ssl, want, "value={value:?}");
        }
    }

    static SHARED_CONFIG_TMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// Hermetic scratch dir for shared-config tests (no `tempfile` dep;
    /// unique per call so parallel tests never share a file).
    fn tmp_aws_dir() -> std::path::PathBuf {
        let n = SHARED_CONFIG_TMP.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("pidash760-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn write_scratch(dir: &std::path::Path, name: &str, text: &str) -> String {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("scratch parent");
        }
        std::fs::write(&path, text).expect("scratch file");
        path.to_str().expect("utf8 path").to_owned()
    }

    fn region_of(map: &HashMap<String, String>) -> Result<String, ConfigError> {
        Ok(Settings::from_map_with(map, Profile::Common, &NoOverlay)?
            .storage
            .region)
    }

    #[test]
    fn s3_region_name_validation_matches_botocore() {
        // `validate_region_name` (`utils.py:1311`): valid host label,
        // max 63 chars, no leading/trailing dash, not all digits. `""`
        // matches (empty repetition) — it fails later at endpoint
        // derivation instead.
        for valid in [
            "",
            "us-east-1",
            "eu-west-1",
            "DD",
            "a",
            "a-b",
            "a1",
            "1a",
            &"a".repeat(63),
        ] {
            assert!(is_valid_region_name(valid), "{valid:?} valid");
        }
        for invalid in [
            "!!",
            "dd # trailing",
            "a b",
            "a_b",
            "a.b",
            "-ab",
            "ab-",
            "-",
            "123",
            "0",
            &"a".repeat(64),
            "a/b",
            "a:b",
        ] {
            assert!(!is_valid_region_name(invalid), "{invalid:?} invalid");
        }
    }

    #[test]
    fn s3_scope_region_shared_config_tail() {
        let dir = tmp_aws_dir();
        let cfg = write_scratch(
            &dir,
            "cfg",
            "[default]\nregion = ca-central-1\n[profile foo]\nregion = ff\n",
        );
        // Neither var set: config-file [default] wins (probe H).
        assert_eq!(
            region_of(&vars(&[("AWS_CONFIG_FILE", cfg.as_str())])).expect("resolves"),
            "ca-central-1"
        );
        // Named profile (probe P13).
        assert_eq!(
            region_of(&vars(&[
                ("AWS_CONFIG_FILE", cfg.as_str()),
                ("AWS_PROFILE", "foo")
            ]))
            .expect("resolves"),
            "ff"
        );
        // `AWS_DEFAULT_PROFILE` wins over `AWS_PROFILE` (probe P1).
        let both = write_scratch(
            &dir,
            "both",
            "[profile a]\nregion = aa\n[profile b]\nregion = bb\n",
        );
        assert_eq!(
            region_of(&vars(&[
                ("AWS_CONFIG_FILE", both.as_str()),
                ("AWS_PROFILE", "a"),
                ("AWS_DEFAULT_PROFILE", "b")
            ]))
            .expect("resolves"),
            "bb"
        );
        // Missing file falls through to us-east-1 (probe P22).
        assert_eq!(
            region_of(&vars(&[(
                "AWS_CONFIG_FILE",
                dir.join("absent").to_str().expect("utf8")
            )]))
            .expect("resolves"),
            "us-east-1"
        );
        // `$HOME/.aws/config` fallback (probe P21).
        let home = write_scratch(&dir, "home/.aws/config", "[default]\nregion = home\n");
        let home_dir = std::path::Path::new(&home)
            .parent()
            .and_then(|p| p.parent())
            .expect("home dir");
        assert_eq!(
            region_of(&vars(&[("HOME", home_dir.to_str().expect("utf8"))])).expect("resolves"),
            "home"
        );
        // Set-empty `AWS_CONFIG_FILE` skips the home file too (probe Q1).
        assert_eq!(
            region_of(&vars(&[
                ("HOME", home_dir.to_str().expect("utf8")),
                ("AWS_CONFIG_FILE", "")
            ]))
            .expect("resolves"),
            "us-east-1"
        );
        // Set-empty file region is verbatim `""` (probe P9; the crash
        // lands downstream at endpoint derivation).
        let empty = write_scratch(&dir, "empty", "[default]\nregion =\n");
        assert_eq!(
            region_of(&vars(&[("AWS_CONFIG_FILE", empty.as_str())])).expect("resolves"),
            ""
        );
        // `[Default]` is ignored but `Region` and `:` are honored
        // (probes P7/P8/P11).
        let quirks = write_scratch(
            &dir,
            "quirks",
            "[Default]\nregion = dd\n[profile q]\nRegion: qq\n",
        );
        assert_eq!(
            region_of(&vars(&[
                ("AWS_CONFIG_FILE", quirks.as_str()),
                ("AWS_PROFILE", "q")
            ]))
            .expect("resolves"),
            "qq"
        );
        assert_eq!(
            region_of(&vars(&[("AWS_CONFIG_FILE", quirks.as_str())])).expect("resolves"),
            "us-east-1"
        );
        // Credentials-only profile validates (probe P5); credentials
        // win per key (probes P6/P20).
        let creds = write_scratch(
            &dir,
            "creds",
            "[foo]\naws_access_key_id = X\n[default]\nregion = cc\n",
        );
        assert_eq!(
            region_of(&vars(&[
                ("AWS_CONFIG_FILE", cfg.as_str()),
                ("AWS_SHARED_CREDENTIALS_FILE", creds.as_str())
            ]))
            .expect("resolves"),
            "cc"
        );
        assert_eq!(
            region_of(&vars(&[
                ("AWS_CONFIG_FILE", cfg.as_str()),
                ("AWS_SHARED_CREDENTIALS_FILE", creds.as_str()),
                ("AWS_PROFILE", "foo")
            ]))
            .expect("resolves"),
            "ff"
        );
        // Env links beat the file (chain order).
        assert_eq!(
            region_of(&vars(&[
                ("AWS_CONFIG_FILE", cfg.as_str()),
                ("AWS_DEFAULT_REGION", "eu-west-1")
            ]))
            .expect("resolves"),
            "eu-west-1"
        );
        assert_eq!(
            region_of(&vars(&[
                ("AWS_CONFIG_FILE", cfg.as_str()),
                ("AWS_REGION", "ap-south-1")
            ]))
            .expect("resolves"),
            "ap-south-1"
        );
    }

    #[test]
    fn s3_scope_region_shared_config_errors_are_loud() {
        let dir = tmp_aws_dir();
        // Malformed config / credentials (probes P3/Q3), duplicate key
        // (P12) / section (P23), garbage line (P24), multi-line region
        // (P15), indented line with no key, keys before any section.
        for (name, text) in [
            ("bad", "[default\nregion = dd\n"),
            ("dupkey", "[default]\nregion = aa\nregion = bb\n"),
            ("dupsec", "[default]\nregion = aa\n[default]\nregion = bb\n"),
            ("garbage", "[default]\nnotakeyvalue\n"),
            ("multiline", "[default]\nregion =\n  dd\n"),
            ("nested", "[default]\nregion =\n  a = b\n"),
            ("indented", "[default]\n  ee\n"),
            ("nosection", "region = dd\n"),
        ] {
            let path = write_scratch(&dir, name, text);
            assert!(
                region_of(&vars(&[("AWS_CONFIG_FILE", path.as_str())])).is_err(),
                "{name} fails"
            );
        }
        let cfg = write_scratch(&dir, "ok", "[default]\nregion = dd\n");
        let bad_creds = write_scratch(&dir, "badcreds", "[default\nx = 1\n");
        assert!(
            region_of(&vars(&[
                ("AWS_CONFIG_FILE", cfg.as_str()),
                ("AWS_SHARED_CREDENTIALS_FILE", bad_creds.as_str())
            ]))
            .is_err(),
            "malformed credentials fail"
        );
        // Set-but-missing profiles fail (probes P2/P14/Q7/Q8),
        // including set-empty and whitespace (no trimming).
        for (var, profile) in [
            ("AWS_PROFILE", "nope"),
            ("AWS_PROFILE", ""),
            ("AWS_PROFILE", "  "),
            ("AWS_DEFAULT_PROFILE", ""),
            ("AWS_PROFILE", "default"),
        ] {
            let only_foo = write_scratch(&dir, "onlyfoo", "[profile foo]\nregion = ff\n");
            assert!(
                region_of(&vars(&[
                    ("AWS_CONFIG_FILE", only_foo.as_str()),
                    (var, profile)
                ]))
                .is_err(),
                "{var}={profile:?} fails"
            );
        }
        // No profile env + no `[default]` section falls through
        // (probes P7/P22), it does not fail.
        let only_foo = write_scratch(&dir, "onlyfoo2", "[profile foo]\nregion = ff\n");
        assert_eq!(
            region_of(&vars(&[("AWS_CONFIG_FILE", only_foo.as_str())])).expect("resolves"),
            "us-east-1"
        );
    }

    #[test]
    fn shared_ini_profile_headers_match_botocore() {
        // Greedy bracket, quoted multiword, unbalanced quotes dropped,
        // the `[profilefoo bar]` quirk, later-section-wins (probe P19),
        // non-profile sections ignored.
        let sections = parse_shared_ini(
            "[preview]\nregion = xx\n\
             [profile foo]\nregion = ff\n\
             [profile \"bar baz\"]\nregion = bb\n\
             [profile broken]\nregion = br\n\
             [profilefoo qux]\nregion = qq\n\
             [default]\nregion = dd\n\
             [profile default]\nregion = pp\n\
             [profile \"unbalanced]\nregion = uu\n",
        )
        .expect("parses");
        let map = config_profile_map(&sections);
        assert_eq!(
            map.get("foo").and_then(|p| p.get("region")).cloned(),
            Some("ff".to_owned())
        );
        assert_eq!(
            map.get("bar baz").and_then(|p| p.get("region")).cloned(),
            Some("bb".to_owned())
        );
        assert_eq!(
            map.get("qux").and_then(|p| p.get("region")).cloned(),
            Some("qq".to_owned())
        );
        assert_eq!(
            map.get("default").and_then(|p| p.get("region")).cloned(),
            Some("pp".to_owned())
        );
        assert!(!map.contains_key("preview"));
        assert_eq!(map.len(), 5, "unbalanced quotes drop the section");
    }

    #[test]
    fn s3_scope_region_shared_config_corners() {
        // Review probes against live botocore 1.34.162: `[DEFAULT]`
        // keys inherit into every parsed section (`cp.get` fallback)
        // but never form a profile; nested validation only applies to
        // empty-base values (`startswith('\n')` gate); leading `~/`
        // expands against the map's `HOME`.
        let dir = tmp_aws_dir();
        // `[DEFAULT]` region feeds an empty `[default]` ...
        let inherited = write_scratch(&dir, "inherited", "[DEFAULT]\nregion = xx\n[default]\n");
        assert_eq!(
            region_of(&vars(&[("AWS_CONFIG_FILE", inherited.as_str())])).expect("resolves"),
            "xx"
        );
        // ... but creates no profile on its own ...
        let defaults_only = write_scratch(&dir, "defaults-only", "[DEFAULT]\nregion = xx\n");
        assert_eq!(
            region_of(&vars(&[("AWS_CONFIG_FILE", defaults_only.as_str())])).expect("resolves"),
            "us-east-1"
        );
        // ... and explicit keys win over it.
        let explicit = write_scratch(
            &dir,
            "explicit",
            "[DEFAULT]\nregion = xx\n[default]\nregion = dd\n",
        );
        assert_eq!(
            region_of(&vars(&[("AWS_CONFIG_FILE", explicit.as_str())])).expect("resolves"),
            "dd"
        );
        // Credentials `[DEFAULT]` merges the same way (per-file only).
        let cfg_empty = write_scratch(&dir, "cfg-empty", "[default]\n");
        let creds_defaults = write_scratch(
            &dir,
            "creds-defaults",
            "[DEFAULT]\nregion = cc\n[default]\n",
        );
        assert_eq!(
            region_of(&vars(&[
                ("AWS_CONFIG_FILE", cfg_empty.as_str()),
                ("AWS_SHARED_CREDENTIALS_FILE", creds_defaults.as_str())
            ]))
            .expect("resolves"),
            "cc"
        );
        // No phantom `DEFAULT` profile: a set profile of that name
        // fails like any other missing profile (`ProfileNotFound`).
        assert!(
            region_of(&vars(&[
                ("AWS_CONFIG_FILE", inherited.as_str()),
                ("AWS_PROFILE", "DEFAULT")
            ]))
            .is_err(),
            "DEFAULT is not a profile"
        );
        // A repeated `[DEFAULT]` merges (no duplicate-section
        // error), but a repeated key still fails loud.
        let repdef = write_scratch(
            &dir,
            "repdef",
            "[DEFAULT]\nregion = xx\n[default]\n[DEFAULT]\noutput = json\n",
        );
        assert_eq!(
            region_of(&vars(&[("AWS_CONFIG_FILE", repdef.as_str())])).expect("resolves"),
            "xx"
        );
        let repdef_dup = write_scratch(
            &dir,
            "repdef-dup",
            "[DEFAULT]\nregion = xx\n[DEFAULT]\nregion = yy\n",
        );
        assert!(
            region_of(&vars(&[("AWS_CONFIG_FILE", repdef_dup.as_str())])).is_err(),
            "repeated DEFAULT key fails"
        );
        // A continuation under a non-empty non-region value is a
        // plain string, not a nested block: no failure.
        let cont = write_scratch(
            &dir,
            "cont",
            "[default]\nregion = dd\noutput = json\n  plaincont\n",
        );
        assert_eq!(
            region_of(&vars(&[("AWS_CONFIG_FILE", cont.as_str())])).expect("resolves"),
            "dd"
        );
        // Leading `~/` in an explicit path expands against `HOME`.
        let home_cfg = write_scratch(&dir, "home2/.aws/config", "[default]\nregion = tilde\n");
        let home_dir = std::path::Path::new(&home_cfg)
            .parent()
            .and_then(|p| p.parent())
            .expect("home dir");
        assert_eq!(
            region_of(&vars(&[
                ("HOME", home_dir.to_str().expect("utf8")),
                ("AWS_CONFIG_FILE", "~/.aws/config")
            ]))
            .expect("resolves"),
            "tilde"
        );
    }

    #[test]
    fn endpoint_protocol_forces_https_only_for_minio_ssl() {
        let base = Settings::test_defaults().storage;
        for (use_minio, ssl, request, want) in [
            (true, true, "http", "https"),
            (true, true, "https", "https"),
            (true, false, "http", "http"),
            (true, false, "https", "https"),
            (false, true, "http", "http"),
            (false, false, "http", "http"),
        ] {
            let storage = StorageSettings {
                use_minio,
                minio_endpoint_ssl: ssl,
                ..base.clone()
            };
            assert_eq!(
                storage.endpoint_protocol(request),
                want,
                "minio={use_minio} ssl={ssl} request={request}"
            );
        }
    }
}
