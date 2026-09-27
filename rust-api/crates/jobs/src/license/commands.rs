//! `configure_instance` + `register_instance` management commands (D-01).
//!
//! Ports `apps/api/pi_dash/license/management/commands/configure_instance.py`
//! and `register_instance.py`. Both commands are pure over small traits
//! ([`Env`], [`SeedStore`], [`InstanceStore`]) so the recorded vectors
//! replay without a database, a broker, or the network; Postgres-backed
//! implementations live next to the logic for the worker to call once this
//! module is wired in. [`super::tasks::delay_message`] is the
//! `instance_traces.delay()` tail of `register_instance` in Celery wire
//! format.
//!
//! # Ported semantics (translate, don't redesign)
//!
//! * `configure_instance` requires the `SECRET_KEY` env var (missing or
//!   empty raises, `configure_instance.py:39-43`).
//! * the seed loop covers the 36 `instance_config_variables`
//!   (`utils/instance_config_variables/core.py`; `extended.py` is empty),
//!   skipping keys that are not db-sourced (`:45-47`), `get_or_create` per
//!   key (`:48`), setting category/encrypted/value + encrypt branch on
//!   create (`:49-57`), warning when the row exists (`:58-59`).
//! * seed values resolve from the environment at run time with the exact
//!   `core.py` defaults (`os.environ.get(key)` → `None` when unset).
//!   Python reads them at module import; a management-command process
//!   imports right before running, so run-time reads are equivalent.
//! * the derived-flag block runs only when NONE of the four
//!   [`DERIVED_FLAG_KEYS`] exists (`:62`), else warns per key
//!   (`:169-170`); the per-key chain is sequential `if`s, not `elif`
//!   (`:64,88,112,140` — harmless, keys distinct — ported as-is).
//! * derived values resolve through the legacy `get_configuration_value`
//!   shim (db row with decrypt, else the caller default) with the exact
//!   caller defaults: `GOOGLE_CLIENT_SECRET` / `GITHUB_CLIENT_SECRET`
//!   default `"0"` (truthy!), `GITLAB_HOST` defaults to
//!   `"https://gitlab.com"` (truthy even when unset); the other inputs
//!   default to `""`.
//! * `register_instance`: `APP_VERSION` env (empty counts as unset) else
//!   `package.json` `version` else `"v0.1.0"` (`:28-38`); latest via the
//!   GitHub releases API else the current version (`:40-52`); create with
//!   `secrets.token_hex(12)` + `IS_TEST == "1"` vs update of
//!   `last_checked_at/current_version/latest_version/is_test/edition`
//!   (`:61-87`); `instance_traces.delay()` always enqueued after either
//!   branch (`:90`).
//! * no HTTP client lives in this crate, so both version probes are
//!   injected ([`PackageJson`], [`LatestProbe`]); the worker supplies the
//!   file read and the GitHub GET at the call site.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use pidash_db::config::accessor::{ConfigError, ConfigRow, ConfigStore};
use pidash_db::config::legacy::{get_configuration_values, LegacyItem};
use pidash_db::config::registry::{ConfigRegistry, ConfigSource};
use pidash_db::config::{ConfigValue, Keyring};

use super::tasks::delay_message;
use crate::celery::CeleryTaskMessage;

/// What the command layer reports as failure. Messages mirror Django's
/// `CommandError` strings exactly.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    /// `"<KEY> env variable is required."`
    /// (`configure_instance.py:43`).
    #[error("{0} env variable is required.")]
    MissingEnv(&'static str),
    /// `"Machine signature is required"` (`register_instance.py:65`).
    #[error("Machine signature is required")]
    MachineSignature,
    /// The store (database) failed.
    #[error("config store failed: {0}")]
    Store(String),
}

impl From<ConfigError> for CommandError {
    fn from(error: ConfigError) -> Self {
        Self::Store(error.to_string())
    }
}

/// Process environment, injectable for tests. Python reads `os.environ`
/// live; the production impl does the same.
pub trait Env {
    fn get(&self, key: &str) -> Option<String>;
}

/// Live process environment.
pub struct ProcessEnv;

impl Env for ProcessEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

// ---------------------------------------------------------------------------
// configure_instance
// ---------------------------------------------------------------------------

/// Derived auth flags seeded as DB rows by `configure_instance`
/// (`configure_instance.py:20`).
pub const DERIVED_FLAG_KEYS: [&str; 4] = [
    "IS_GOOGLE_ENABLED",
    "IS_GITHUB_ENABLED",
    "IS_GITLAB_ENABLED",
    "IS_GITEA_ENABLED",
];

/// One `instance_config_variables` entry: the key, its category, whether
/// the value is Fernet-encrypted at rest, and the `core.py` env default
/// (`None` = bare `os.environ.get(key)`).
pub struct SeedVariable {
    pub key: &'static str,
    pub category: &'static str,
    pub is_encrypted: bool,
    pub default: Option<&'static str>,
}

/// The 36 seeded variables in `core.py` order
/// (`instance_config_variables/__init__.py:7-8`).
pub const SEED_VARIABLES: [SeedVariable; 36] = [
    SeedVariable {
        key: "ENABLE_SIGNUP",
        category: "AUTHENTICATION",
        is_encrypted: false,
        default: Some("1"),
    },
    SeedVariable {
        key: "ENABLE_EMAIL_PASSWORD",
        category: "AUTHENTICATION",
        is_encrypted: false,
        default: Some("1"),
    },
    SeedVariable {
        key: "ENABLE_MAGIC_LINK_LOGIN",
        category: "AUTHENTICATION",
        is_encrypted: false,
        default: Some("0"),
    },
    SeedVariable {
        key: "DISABLE_WORKSPACE_CREATION",
        category: "WORKSPACE_MANAGEMENT",
        is_encrypted: false,
        default: Some("0"),
    },
    SeedVariable {
        key: "GOOGLE_CLIENT_ID",
        category: "GOOGLE",
        is_encrypted: false,
        default: None,
    },
    SeedVariable {
        key: "GOOGLE_CLIENT_SECRET",
        category: "GOOGLE",
        is_encrypted: true,
        default: None,
    },
    SeedVariable {
        key: "ENABLE_GOOGLE_SYNC",
        category: "GOOGLE",
        is_encrypted: false,
        default: Some("0"),
    },
    SeedVariable {
        key: "GITHUB_APP_ID",
        category: "GITHUB",
        is_encrypted: false,
        default: None,
    },
    SeedVariable {
        key: "GITHUB_APP_SLUG",
        category: "GITHUB",
        is_encrypted: false,
        default: None,
    },
    SeedVariable {
        key: "GITHUB_APP_CLIENT_ID",
        category: "GITHUB",
        is_encrypted: false,
        default: None,
    },
    SeedVariable {
        key: "GITHUB_CLIENT_ID",
        category: "GITHUB",
        is_encrypted: false,
        default: None,
    },
    SeedVariable {
        key: "GITHUB_CLIENT_SECRET",
        category: "GITHUB",
        is_encrypted: true,
        default: None,
    },
    SeedVariable {
        key: "GITHUB_ORGANIZATION_ID",
        category: "GITHUB",
        is_encrypted: false,
        default: None,
    },
    SeedVariable {
        key: "ENABLE_GITHUB_SYNC",
        category: "GITHUB",
        is_encrypted: false,
        default: Some("0"),
    },
    SeedVariable {
        key: "GITLAB_HOST",
        category: "GITLAB",
        is_encrypted: false,
        default: None,
    },
    SeedVariable {
        key: "GITLAB_CLIENT_ID",
        category: "GITLAB",
        is_encrypted: false,
        default: None,
    },
    SeedVariable {
        key: "GITLAB_CLIENT_SECRET",
        category: "GITLAB",
        is_encrypted: true,
        default: None,
    },
    SeedVariable {
        key: "ENABLE_GITLAB_SYNC",
        category: "GITLAB",
        is_encrypted: false,
        default: Some("0"),
    },
    // NOTE (ported quirk): `IS_GITEA_ENABLED` is both a seed variable and
    // a derived flag. On a fresh database the seed loop creates it, so the
    // derived block's any-exists gate is already tripped and
    // `IS_GOOGLE/IS_GITHUB/IS_GITLAB_ENABLED` are never seeded. Kept as-is.
    SeedVariable {
        key: "IS_GITEA_ENABLED",
        category: "GITEA",
        is_encrypted: false,
        default: Some("0"),
    },
    SeedVariable {
        key: "GITEA_HOST",
        category: "GITEA",
        is_encrypted: false,
        default: None,
    },
    SeedVariable {
        key: "GITEA_CLIENT_ID",
        category: "GITEA",
        is_encrypted: false,
        default: None,
    },
    SeedVariable {
        key: "GITEA_CLIENT_SECRET",
        category: "GITEA",
        is_encrypted: true,
        default: None,
    },
    SeedVariable {
        key: "ENABLE_GITEA_SYNC",
        category: "GITEA",
        is_encrypted: false,
        default: Some("0"),
    },
    SeedVariable {
        key: "ENABLE_SMTP",
        category: "SMTP",
        is_encrypted: false,
        default: Some("0"),
    },
    SeedVariable {
        key: "EMAIL_HOST",
        category: "SMTP",
        is_encrypted: false,
        default: Some(""),
    },
    SeedVariable {
        key: "EMAIL_HOST_USER",
        category: "SMTP",
        is_encrypted: false,
        default: Some(""),
    },
    SeedVariable {
        key: "EMAIL_HOST_PASSWORD",
        category: "SMTP",
        is_encrypted: true,
        default: Some(""),
    },
    SeedVariable {
        key: "EMAIL_PORT",
        category: "SMTP",
        is_encrypted: false,
        default: Some("587"),
    },
    SeedVariable {
        key: "EMAIL_FROM",
        category: "SMTP",
        is_encrypted: false,
        default: Some(""),
    },
    SeedVariable {
        key: "EMAIL_USE_TLS",
        category: "SMTP",
        is_encrypted: false,
        default: Some("1"),
    },
    SeedVariable {
        key: "EMAIL_USE_SSL",
        category: "SMTP",
        is_encrypted: false,
        default: Some("0"),
    },
    SeedVariable {
        key: "LLM_API_KEY",
        category: "AI",
        is_encrypted: true,
        default: None,
    },
    SeedVariable {
        key: "LLM_PROVIDER",
        category: "AI",
        is_encrypted: false,
        default: Some("openai"),
    },
    SeedVariable {
        key: "LLM_MODEL",
        category: "AI",
        is_encrypted: false,
        default: Some("gpt-4o-mini"),
    },
    SeedVariable {
        key: "GPT_ENGINE",
        category: "AI",
        is_encrypted: false,
        default: Some("gpt-3.5-turbo"),
    },
    SeedVariable {
        key: "UNSPLASH_ACCESS_KEY",
        category: "UNSPLASH",
        is_encrypted: true,
        default: Some(""),
    },
];

/// Only db-sourced keys are seeded (`_is_db_sourced`,
/// `configure_instance.py:23-29`); env-sourced keys never get a DB row,
/// and unregistered keys keep the legacy seed behavior.
pub fn is_db_sourced(registry: &ConfigRegistry, key: &str) -> bool {
    registry
        .get(key)
        .map(|entry| entry.source == ConfigSource::Db)
        .unwrap_or(true)
}

/// A row to create: Django's `get_or_create(key=…)` plus the fields set on
/// the created object before `save()`.
pub struct NewConfigRow {
    pub key: String,
    pub value: Option<String>,
    pub category: String,
    pub is_encrypted: bool,
}

/// The `instance_configurations` writes `configure_instance` needs, over
/// the shared [`ConfigStore`] reads (which the derived-flag resolution
/// reuses through the legacy shim instead of forking it).
pub trait SeedStore: ConfigStore {
    /// `get_or_create(key)`: the existing row with `false`, or the
    /// inserted `seed` with `true`. Existing rows are never modified.
    fn get_or_create(
        &self,
        seed: NewConfigRow,
    ) -> impl std::future::Future<Output = Result<(ConfigRow, bool), CommandError>> + Send;
    /// Direct insert for derived flags (the any-exists gate runs first).
    fn insert(
        &self,
        row: NewConfigRow,
    ) -> impl std::future::Future<Output = Result<(), CommandError>> + Send;
    /// `filter(key__in=DERIVED_FLAG_KEYS).exists()`.
    fn derived_present(
        &self,
    ) -> impl std::future::Future<Output = Result<bool, CommandError>> + Send;
}

/// One stdout line, keeping Django's SUCCESS vs WARNING split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineKind {
    Success,
    Warning,
}

/// One stdout line of the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLine {
    pub kind: LineKind,
    pub text: String,
}

/// The command's stdout lines, in emission order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfigureReport {
    pub lines: Vec<OutputLine>,
}

impl ConfigureReport {
    fn success(&mut self, text: String) {
        self.lines.push(OutputLine {
            kind: LineKind::Success,
            text,
        });
    }

    fn warning(&mut self, text: String) {
        self.lines.push(OutputLine {
            kind: LineKind::Warning,
            text,
        });
    }

    /// Keys created with the "loaded with value" message, in order.
    pub fn seeded_keys(&self) -> Vec<&str> {
        self.lines
            .iter()
            .filter(|line| {
                line.kind == LineKind::Success
                    && line
                        .text
                        .ends_with(" loaded with value from environment variable.")
            })
            .map(|line| {
                line.text
                    .strip_suffix(" loaded with value from environment variable.")
                    .expect("suffix checked")
            })
            .collect()
    }
}

/// Python `bool(x)` over resolved config values: empty string, `Null`,
/// zero, and `False` are falsy; everything else is truthy.
fn is_truthy(value: &ConfigValue) -> bool {
    match value {
        ConfigValue::Null => false,
        ConfigValue::Str(s) => !s.is_empty(),
        ConfigValue::Bool(b) => *b,
        ConfigValue::Int(n) => *n != 0,
        ConfigValue::Float(f) => *f != 0.0 && !f.is_nan(),
    }
}

/// Resolve the legacy `[{key, default}]` items exactly like
/// `get_configuration_value`: db rows (decrypted) win, else the caller
/// default. The caller defaults come from the injected [`Env`] with the
/// exact `os.environ.get(…, …)` fallbacks at the Python call sites.
async fn legacy_values<S: SeedStore>(
    store: &S,
    registry: &ConfigRegistry,
    keyring: &Keyring,
    items: &[(String, Option<String>)],
) -> Result<Vec<ConfigValue>, CommandError> {
    let legacy: Vec<LegacyItem> = items
        .iter()
        .map(|(key, default)| {
            LegacyItem::new(
                key.clone(),
                match default {
                    Some(value) => ConfigValue::Str(value.clone()),
                    None => ConfigValue::Null,
                },
            )
        })
        .collect();
    Ok(get_configuration_values(registry, store, keyring, &legacy).await?)
}

/// Seed the instance configuration, exactly like the management command.
pub async fn configure_instance<E: Env, S: SeedStore>(
    env: &E,
    store: &S,
    registry: &ConfigRegistry,
    keyring: &Keyring,
) -> Result<ConfigureReport, CommandError> {
    // Mandatory keys first (`configure_instance.py:39-43`).
    match env.get("SECRET_KEY") {
        Some(secret) if !secret.is_empty() => {}
        _ => return Err(CommandError::MissingEnv("SECRET_KEY")),
    }

    let mut report = ConfigureReport::default();

    for var in &SEED_VARIABLES {
        if !is_db_sourced(registry, var.key) {
            continue;
        }
        let raw = env.get(var.key).or_else(|| var.default.map(str::to_string));
        // `encrypt_data(value)`: `None`/empty yields `""`, else Fernet.
        let value = if var.is_encrypted {
            Some(keyring.encrypt(raw.as_deref().unwrap_or_default()))
        } else {
            raw
        };
        let (_, created) = store
            .get_or_create(NewConfigRow {
                key: var.key.to_string(),
                value,
                category: var.category.to_string(),
                is_encrypted: var.is_encrypted,
            })
            .await?;
        if created {
            report.success(format!(
                "{} loaded with value from environment variable.",
                var.key
            ));
        } else {
            report.warning(format!("{} configuration already exists", var.key));
        }
    }

    if !store.derived_present().await? {
        // `for key in keys` with sequential `if`s, not `elif` — ported
        // as-is (`configure_instance.py:63-168`; BUG-7: harmless, keys distinct).
        for key in &DERIVED_FLAG_KEYS {
            if *key == "IS_GOOGLE_ENABLED" {
                let values = legacy_values(
                    store,
                    registry,
                    keyring,
                    &[
                        (
                            "GOOGLE_CLIENT_ID".to_string(),
                            Some(env.get("GOOGLE_CLIENT_ID").unwrap_or_default()),
                        ),
                        (
                            "GOOGLE_CLIENT_SECRET".to_string(),
                            Some(
                                env.get("GOOGLE_CLIENT_SECRET")
                                    .unwrap_or_else(|| "0".to_string()),
                            ),
                        ),
                    ],
                )
                .await?;
                // `GOOGLE_CLIENT_SECRET` defaults to `"0"` — a truthy string —
                // so the flag is `"1"` when only the client id is set.
                let value = if is_truthy(&values[0]) && is_truthy(&values[1]) {
                    "1"
                } else {
                    "0"
                };
                store
                    .insert(NewConfigRow {
                        key: "IS_GOOGLE_ENABLED".to_string(),
                        value: Some(value.to_string()),
                        category: "AUTHENTICATION".to_string(),
                        is_encrypted: false,
                    })
                    .await?;
                report.success(
                    "IS_GOOGLE_ENABLED loaded with value from environment variable.".to_string(),
                );
            }
            if *key == "IS_GITHUB_ENABLED" {
                let values = legacy_values(
                    store,
                    registry,
                    keyring,
                    &[
                        (
                            "GITHUB_CLIENT_ID".to_string(),
                            Some(env.get("GITHUB_CLIENT_ID").unwrap_or_default()),
                        ),
                        (
                            "GITHUB_CLIENT_SECRET".to_string(),
                            Some(
                                env.get("GITHUB_CLIENT_SECRET")
                                    .unwrap_or_else(|| "0".to_string()),
                            ),
                        ),
                    ],
                )
                .await?;
                let value = if is_truthy(&values[0]) && is_truthy(&values[1]) {
                    "1"
                } else {
                    "0"
                };
                store
                    .insert(NewConfigRow {
                        key: "IS_GITHUB_ENABLED".to_string(),
                        value: Some(value.to_string()),
                        category: "AUTHENTICATION".to_string(),
                        is_encrypted: false,
                    })
                    .await?;
                report.success(
                    "IS_GITHUB_ENABLED loaded with value from environment variable.".to_string(),
                );
            }
            if *key == "IS_GITLAB_ENABLED" {
                let values = legacy_values(
                    store,
                    registry,
                    keyring,
                    &[
                        (
                            "GITLAB_HOST".to_string(),
                            Some(
                                env.get("GITLAB_HOST")
                                    .unwrap_or_else(|| "https://gitlab.com".to_string()),
                            ),
                        ),
                        (
                            "GITLAB_CLIENT_ID".to_string(),
                            Some(env.get("GITLAB_CLIENT_ID").unwrap_or_default()),
                        ),
                        (
                            "GITLAB_CLIENT_SECRET".to_string(),
                            Some(env.get("GITLAB_CLIENT_SECRET").unwrap_or_default()),
                        ),
                    ],
                )
                .await?;
                // `GITLAB_HOST` defaults to `"https://gitlab.com"`, truthy even
                // when unset — the flag still needs id + secret.
                let value =
                    if is_truthy(&values[0]) && is_truthy(&values[1]) && is_truthy(&values[2]) {
                        "1"
                    } else {
                        "0"
                    };
                store
                    .insert(NewConfigRow {
                        key: "IS_GITLAB_ENABLED".to_string(),
                        value: Some(value.to_string()),
                        category: "AUTHENTICATION".to_string(),
                        is_encrypted: false,
                    })
                    .await?;
                report.success(
                    "IS_GITLAB_ENABLED loaded with value from environment variable.".to_string(),
                );
            }
            if *key == "IS_GITEA_ENABLED" {
                let values = legacy_values(
                    store,
                    registry,
                    keyring,
                    &[
                        (
                            "GITEA_HOST".to_string(),
                            Some(env.get("GITEA_HOST").unwrap_or_default()),
                        ),
                        (
                            "GITEA_CLIENT_ID".to_string(),
                            Some(env.get("GITEA_CLIENT_ID").unwrap_or_default()),
                        ),
                        (
                            "GITEA_CLIENT_SECRET".to_string(),
                            Some(env.get("GITEA_CLIENT_SECRET").unwrap_or_default()),
                        ),
                    ],
                )
                .await?;
                let value =
                    if is_truthy(&values[0]) && is_truthy(&values[1]) && is_truthy(&values[2]) {
                        "1"
                    } else {
                        "0"
                    };
                store
                    .insert(NewConfigRow {
                        key: "IS_GITEA_ENABLED".to_string(),
                        value: Some(value.to_string()),
                        category: "AUTHENTICATION".to_string(),
                        is_encrypted: false,
                    })
                    .await?;
                report.success(
                    "IS_GITEA_ENABLED loaded with value from environment variable.".to_string(),
                );
            }
        }
    } else {
        for key in &DERIVED_FLAG_KEYS {
            report.warning(format!("{key} configuration already exists"));
        }
    }

    Ok(report)
}

// ---------------------------------------------------------------------------
// register_instance
// ---------------------------------------------------------------------------

/// Fallback when neither `APP_VERSION` nor `package.json` yields a version
/// (`register_instance.py:38`).
pub const DEFAULT_VERSION: &str = "v0.1.0";

/// GitHub endpoint for the latest-release probe (`:43-46`).
pub const RELEASES_URL: &str =
    "https://api.github.com/repos/The-AI-Republic/pi-dash/releases/latest";

/// Edition stored on the instance row (`:76,86`).
pub const COMMUNITY_EDITION: &str = "PI_DASH_COMMUNITY";

/// The `package.json` version input: unreadable-or-unparseable files print
/// the error line and fall back, while a parsed file without a `version`
/// key falls back silently (`:33-38`).
pub enum PackageJson {
    Unreadable,
    Parsed(Option<String>),
}

/// The latest-release probe: a fetched `tag_name` (`None` = key missing,
/// silent fallback) or a request failure (prints the error line,
/// `:51-52`).
pub enum LatestProbe {
    Tag(Option<String>),
    Failed,
}

/// Resolve the current version: `APP_VERSION` env (empty counts as unset,
/// like Python's falsiness test, `:28-29`), else `package.json`, else
/// `"v0.1.0"`. Appends stdout lines for the failure path only.
pub fn resolve_current_version(
    app_version: Option<&str>,
    package: &PackageJson,
    stdout: &mut Vec<String>,
) -> String {
    if let Some(version) = app_version {
        if !version.is_empty() {
            return version.to_string();
        }
    }
    match package {
        PackageJson::Parsed(version) => version
            .clone()
            .unwrap_or_else(|| DEFAULT_VERSION.to_string()),
        PackageJson::Unreadable => {
            stdout.push("Error checking for current version".to_string());
            DEFAULT_VERSION.to_string()
        }
    }
}

/// Resolve the latest version: the release `tag_name`, else the current
/// version; a failed request prints the error line first (`:40-52`).
pub fn resolve_latest_version(
    probe: LatestProbe,
    fallback: &str,
    stdout: &mut Vec<String>,
) -> String {
    match probe {
        LatestProbe::Tag(tag) => tag.unwrap_or_else(|| fallback.to_string()),
        LatestProbe::Failed => {
            stdout.push("Error checking for latest version".to_string());
            fallback.to_string()
        }
    }
}

/// `secrets.token_hex(12)`: 12 CSPRNG bytes as 24 lowercase hex chars.
/// The bytes come from a v4 UUID (CSPRNG-backed); the wire shape — 24 hex
/// chars — is identical, and uniqueness is equivalent in practice.
pub fn generate_instance_token() -> String {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    let mut token = String::with_capacity(24);
    for byte in &bytes[..12] {
        token.push_str(&format!("{byte:02x}"));
    }
    token
}

/// The stored instance row, as the command reads and writes it.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredInstance {
    pub id: uuid::Uuid,
    pub instance_id: String,
}

/// The `instances` writes `register_instance` needs.
pub trait InstanceStore {
    /// `Instance.objects.first()`.
    fn first(
        &self,
    ) -> impl std::future::Future<Output = Result<Option<StoredInstance>, CommandError>> + Send;
    /// Create the registered row (`:68-77`; Django fills the remaining
    /// columns with their field defaults — see [`PgInstanceStore`]).
    fn create(
        &self,
        input: InstanceInput,
    ) -> impl std::future::Future<Output = Result<StoredInstance, CommandError>> + Send;
    /// The update branch (`:82-87`): only these columns change.
    fn update_check(
        &self,
        id: uuid::Uuid,
        current_version: &str,
        latest_version: Option<&str>,
        is_test: bool,
        checked_at: DateTime<Utc>,
    ) -> impl std::future::Future<Output = Result<(), CommandError>> + Send;
}

/// The created row's caller-supplied columns.
pub struct InstanceInput {
    pub instance_name: String,
    pub instance_id: String,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub is_test: bool,
    pub checked_at: DateTime<Utc>,
}

/// The command's outcome: stdout lines plus the always-enqueued
/// `instance_traces` delay message (`:90`).
#[derive(Debug, PartialEq)]
pub struct RegisterReport {
    pub created: bool,
    pub instance_id: String,
    pub current_version: String,
    pub latest_version: String,
    pub stdout: Vec<String>,
    pub delay: CeleryTaskMessage,
}

/// Check-then-register, exactly like the management command.
pub async fn register_instance<E: Env, S: InstanceStore>(
    env: &E,
    store: &S,
    machine_signature: Option<&str>,
    package: &PackageJson,
    latest: LatestProbe,
    now: DateTime<Utc>,
) -> Result<RegisterReport, CommandError> {
    let mut stdout = Vec::new();
    let current_version =
        resolve_current_version(env.get("APP_VERSION").as_deref(), package, &mut stdout);
    let latest_version = resolve_latest_version(latest, &current_version, &mut stdout);
    let is_test = env.get("IS_TEST").as_deref() == Some("1");

    let (created, instance_id) = match store.first().await? {
        None => {
            // `options.get("machine_signature", "machine-signature")`
            // (`:62`); falsy raises (`:64-65`) — reachable only as `""`.
            let signature = machine_signature.unwrap_or("machine-signature");
            if signature.is_empty() {
                return Err(CommandError::MachineSignature);
            }
            let instance_id = generate_instance_token();
            store
                .create(InstanceInput {
                    instance_name: "Pi Dash Community Edition".to_string(),
                    instance_id: instance_id.clone(),
                    current_version: current_version.clone(),
                    latest_version: Some(latest_version.clone()),
                    is_test,
                    checked_at: now,
                })
                .await?;
            stdout.push("Instance registered".to_string());
            (true, instance_id)
        }
        Some(existing) => {
            stdout.push("Instance already registered".to_string());
            store
                .update_check(
                    existing.id,
                    &current_version,
                    Some(&latest_version),
                    is_test,
                    now,
                )
                .await?;
            (false, existing.instance_id)
        }
    };

    Ok(RegisterReport {
        created,
        instance_id,
        current_version: current_version.clone(),
        latest_version,
        stdout,
        delay: delay_message(),
    })
}

// ---------------------------------------------------------------------------
// Postgres stores (for the worker once this module is wired in)
// ---------------------------------------------------------------------------

/// Postgres [`SeedStore`]: the exact Django query shapes. Reads use the
/// default-manager scope (`deleted_at IS NULL`); note the unique index on
/// `key` is soft-delete-unaware (ported quirk from the models layer), so a
/// soft-deleted row still blocks re-insert — `get_or_create` then resolves
/// to the conflicting row as not-created, mirroring Django's
/// IntegrityError-retry resolving to the same row.
pub struct PgSeedStore {
    pool: sqlx::PgPool,
}

impl PgSeedStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    /// The keyed lookup over `instance_configurations`, nothing else.
    pub const FETCH_SQL: &'static str =
        "SELECT key, value, is_encrypted FROM instance_configurations WHERE key = ANY($1)";
}

impl ConfigStore for PgSeedStore {
    async fn fetch(&self, keys: &[&str]) -> Result<HashMap<String, ConfigRow>, ConfigError> {
        let rows: Vec<(String, Option<String>, bool)> = sqlx::query_as(Self::FETCH_SQL)
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

impl SeedStore for PgSeedStore {
    async fn get_or_create(&self, seed: NewConfigRow) -> Result<(ConfigRow, bool), CommandError> {
        let existing: Option<(Option<String>, bool)> = sqlx::query_as(
            "SELECT value, is_encrypted FROM instance_configurations WHERE key = $1 AND deleted_at IS NULL",
        )
        .bind(&seed.key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CommandError::Store(e.to_string()))?;
        if let Some((value, is_encrypted)) = existing {
            return Ok((
                ConfigRow {
                    value,
                    is_encrypted,
                },
                false,
            ));
        }
        let inserted: Option<String> = sqlx::query_scalar(
            "INSERT INTO instance_configurations (id, created_at, updated_at, key, value, category, is_encrypted) VALUES ($1, now(), now(), $2, $3, $4, $5) ON CONFLICT (key) DO NOTHING RETURNING key",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(&seed.key)
        .bind(&seed.value)
        .bind(&seed.category)
        .bind(seed.is_encrypted)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| CommandError::Store(e.to_string()))?;
        if inserted.is_some() {
            Ok((
                ConfigRow {
                    value: seed.value,
                    is_encrypted: seed.is_encrypted,
                },
                true,
            ))
        } else {
            // Lost a race (or hit a soft-deleted row shadowing the key):
            // resolve to the conflicting row, like Django's retry-`get`.
            let row: (Option<String>, bool) = sqlx::query_as(
                "SELECT value, is_encrypted FROM instance_configurations WHERE key = $1",
            )
            .bind(&seed.key)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| CommandError::Store(e.to_string()))?;
            Ok((
                ConfigRow {
                    value: row.0,
                    is_encrypted: row.1,
                },
                false,
            ))
        }
    }

    async fn insert(&self, row: NewConfigRow) -> Result<(), CommandError> {
        sqlx::query(
            "INSERT INTO instance_configurations (id, created_at, updated_at, key, value, category, is_encrypted) VALUES ($1, now(), now(), $2, $3, $4, $5)",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(&row.key)
        .bind(&row.value)
        .bind(&row.category)
        .bind(row.is_encrypted)
        .execute(&self.pool)
        .await
        .map_err(|e| CommandError::Store(e.to_string()))?;
        Ok(())
    }

    async fn derived_present(&self) -> Result<bool, CommandError> {
        let keys: Vec<&str> = DERIVED_FLAG_KEYS.to_vec();
        let present: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM instance_configurations WHERE key = ANY($1) AND deleted_at IS NULL)",
        )
        .bind(&keys)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| CommandError::Store(e.to_string()))?;
        Ok(present)
    }
}

/// Postgres [`InstanceStore`].
pub struct PgInstanceStore {
    pool: sqlx::PgPool,
}

impl PgInstanceStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

impl InstanceStore for PgInstanceStore {
    async fn first(&self) -> Result<Option<StoredInstance>, CommandError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_db::license::queries::INSTANCE_FIRST_SQL)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| CommandError::Store(e.to_string()))?;
        row.map(|row| {
            use sqlx::Row;
            Ok(StoredInstance {
                id: row
                    .try_get("id")
                    .map_err(|e| CommandError::Store(e.to_string()))?,
                instance_id: row
                    .try_get("instance_id")
                    .map_err(|e| CommandError::Store(e.to_string()))?,
            })
        })
        .transpose()
    }

    async fn create(&self, input: InstanceInput) -> Result<StoredInstance, CommandError> {
        // `Instance.objects.create(…)` with every other column at its
        // Django field default (`instance.py:22-50`): `domain` stores `""`
        // (blank, not null), telemetry/support default true, setup/visited/
        // verified/test-deprecated default false, audit ids null.
        let id = uuid::Uuid::new_v4();
        sqlx::query(
            "INSERT INTO instances (id, created_at, updated_at, instance_name, instance_id, current_version, latest_version, edition, domain, last_checked_at, is_telemetry_enabled, is_support_required, is_setup_done, is_signup_screen_visited, is_verified, is_test, is_current_version_deprecated) VALUES ($1, now(), now(), $2, $3, $4, $5, $6, '', $7, true, true, false, false, false, $8, false)",
        )
        .bind(id)
        .bind(&input.instance_name)
        .bind(&input.instance_id)
        .bind(&input.current_version)
        .bind(&input.latest_version)
        .bind(COMMUNITY_EDITION)
        .bind(input.checked_at)
        .bind(input.is_test)
        .execute(&self.pool)
        .await
        .map_err(|e| CommandError::Store(e.to_string()))?;
        Ok(StoredInstance {
            id,
            instance_id: input.instance_id,
        })
    }

    async fn update_check(
        &self,
        id: uuid::Uuid,
        current_version: &str,
        latest_version: Option<&str>,
        is_test: bool,
        checked_at: DateTime<Utc>,
    ) -> Result<(), CommandError> {
        // `instance.save()` rewrites every column; the unchanged ones go
        // back with identical values (unobservable), so only the five
        // changed columns plus the `auto_now` bump are emitted here.
        sqlx::query(
            "UPDATE instances SET last_checked_at = $2, current_version = $3, latest_version = $4, is_test = $5, edition = $6, updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(checked_at)
        .bind(current_version)
        .bind(latest_version)
        .bind(is_test)
        .bind(COMMUNITY_EDITION)
        .execute(&self.pool)
        .await
        .map_err(|e| CommandError::Store(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> serde_json::Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/license/tasks")
            .join(name);
        let text = std::fs::read_to_string(&path).expect("task fixture exists");
        serde_json::from_str(&text).expect("task fixture is valid JSON")
    }

    /// Deterministic registry: no `ENV_KEYS_OVERRIDE_VAR` leakage.
    fn registry() -> ConfigRegistry {
        ConfigRegistry::build_with_overrides(HashMap::new())
    }

    fn keyring() -> Keyring {
        Keyring::from_secret("test-secret-key")
    }

    #[derive(Default)]
    struct MapEnv {
        vars: HashMap<String, String>,
    }

    impl MapEnv {
        fn with(mut self, key: &str, value: &str) -> Self {
            self.vars.insert(key.to_string(), value.to_string());
            self
        }
    }

    impl Env for MapEnv {
        fn get(&self, key: &str) -> Option<String> {
            self.vars.get(key).cloned()
        }
    }

    #[derive(Debug, Clone)]
    struct MemRow {
        value: Option<String>,
        category: String,
        is_encrypted: bool,
    }

    #[derive(Default)]
    struct MemSeed {
        rows: std::sync::Mutex<HashMap<String, MemRow>>,
    }

    impl MemSeed {
        fn get_row(&self, key: &str) -> Option<MemRow> {
            self.rows.lock().expect("test lock").get(key).cloned()
        }

        fn has_row(&self, key: &str) -> bool {
            self.rows.lock().expect("test lock").contains_key(key)
        }

        fn put_row(&self, key: &str, row: MemRow) {
            self.rows
                .lock()
                .expect("test lock")
                .insert(key.to_string(), row);
        }
    }

    impl ConfigStore for MemSeed {
        async fn fetch(&self, keys: &[&str]) -> Result<HashMap<String, ConfigRow>, ConfigError> {
            let rows = self.rows.lock().expect("test lock");
            Ok(keys
                .iter()
                .filter_map(|key| {
                    rows.get(*key).map(|row| {
                        (
                            (*key).to_string(),
                            ConfigRow {
                                value: row.value.clone(),
                                is_encrypted: row.is_encrypted,
                            },
                        )
                    })
                })
                .collect())
        }
    }

    impl SeedStore for MemSeed {
        async fn get_or_create(
            &self,
            seed: NewConfigRow,
        ) -> Result<(ConfigRow, bool), CommandError> {
            if let Some(existing) = self.get_row(&seed.key) {
                return Ok((
                    ConfigRow {
                        value: existing.value.clone(),
                        is_encrypted: existing.is_encrypted,
                    },
                    false,
                ));
            }
            self.put_row(
                &seed.key,
                MemRow {
                    value: seed.value.clone(),
                    category: seed.category.clone(),
                    is_encrypted: seed.is_encrypted,
                },
            );
            Ok((
                ConfigRow {
                    value: seed.value,
                    is_encrypted: seed.is_encrypted,
                },
                true,
            ))
        }

        async fn insert(&self, row: NewConfigRow) -> Result<(), CommandError> {
            self.put_row(
                &row.key,
                MemRow {
                    value: row.value,
                    category: row.category,
                    is_encrypted: row.is_encrypted,
                },
            );
            Ok(())
        }

        async fn derived_present(&self) -> Result<bool, CommandError> {
            Ok(DERIVED_FLAG_KEYS.iter().any(|key| self.has_row(key)))
        }
    }

    fn block<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("test runtime")
            .block_on(future)
    }

    // -- seed table ---------------------------------------------------------

    #[test]
    fn seed_table_has_36_variables_in_core_order() {
        assert_eq!(SEED_VARIABLES.len(), 36);
        let keys: Vec<&str> = SEED_VARIABLES.iter().map(|var| var.key).collect();
        assert_eq!(
            &keys[..4],
            [
                "ENABLE_SIGNUP",
                "ENABLE_EMAIL_PASSWORD",
                "ENABLE_MAGIC_LINK_LOGIN",
                "DISABLE_WORKSPACE_CREATION"
            ]
        );
        assert!(keys.contains(&"GOOGLE_CLIENT_SECRET"));
        assert!(keys.contains(&"GITHUB_CLIENT_SECRET"));
        assert!(keys.contains(&"GITLAB_CLIENT_SECRET"));
        assert!(keys.contains(&"GITEA_CLIENT_SECRET"));
        assert!(keys.contains(&"EMAIL_HOST_PASSWORD"));
        assert!(keys.contains(&"LLM_API_KEY"));
        assert!(keys.contains(&"UNSPLASH_ACCESS_KEY"));
        // `IS_GITEA_ENABLED` is both seeded and derived (ported quirk).
        assert!(keys.contains(&"IS_GITEA_ENABLED"));
        assert_eq!(
            DERIVED_FLAG_KEYS,
            [
                "IS_GOOGLE_ENABLED",
                "IS_GITHUB_ENABLED",
                "IS_GITLAB_ENABLED",
                "IS_GITEA_ENABLED",
            ]
        );
        let recorded: Vec<String> = fixture("configure_instance.golden.json")["derived_flag_keys"]
            .as_array()
            .expect("derived keys")
            .iter()
            .map(|v| v.as_str().expect("key").to_string())
            .collect();
        assert_eq!(recorded, DERIVED_FLAG_KEYS);
    }

    #[test]
    fn secret_key_is_mandatory() {
        let store = MemSeed::default();
        let registry = registry();
        let env = MapEnv::default();
        assert_eq!(
            block(configure_instance(&env, &store, &registry, &keyring())),
            Err(CommandError::MissingEnv("SECRET_KEY"))
        );
        let env = MapEnv::default().with("SECRET_KEY", "");
        assert_eq!(
            block(configure_instance(&env, &store, &registry, &keyring())),
            Err(CommandError::MissingEnv("SECRET_KEY"))
        );
    }

    #[test]
    fn fresh_seed_creates_36_rows_then_skips_derived_as_ported() {
        let store = MemSeed::default();
        let registry = registry();
        let env = MapEnv::default().with("SECRET_KEY", "s3cret");
        let report = block(configure_instance(&env, &store, &registry, &keyring()))
            .expect("configure succeeds");
        // All 36 seeded, in table order.
        assert_eq!(report.seeded_keys().len(), 36);
        assert_eq!(
            report.seeded_keys()[..4].to_vec(),
            vec![
                "ENABLE_SIGNUP",
                "ENABLE_EMAIL_PASSWORD",
                "ENABLE_MAGIC_LINK_LOGIN",
                "DISABLE_WORKSPACE_CREATION"
            ]
        );
        assert_eq!(
            store.get_row("ENABLE_SIGNUP").expect("seeded").category,
            "AUTHENTICATION"
        );
        assert_eq!(
            store
                .get_row("ENABLE_SIGNUP")
                .expect("seeded")
                .value
                .as_deref(),
            Some("1")
        );
        // The ported quirk: the seed loop just created IS_GITEA_ENABLED, so
        // the any-exists gate trips and the derived block warns per key —
        // IS_GOOGLE/IS_GITHUB/IS_GITLAB_ENABLED are never seeded.
        let warnings: Vec<&str> = report
            .lines
            .iter()
            .filter(|line| line.kind == LineKind::Warning)
            .map(|line| line.text.as_str())
            .collect();
        assert_eq!(
            warnings,
            [
                "IS_GOOGLE_ENABLED configuration already exists",
                "IS_GITHUB_ENABLED configuration already exists",
                "IS_GITLAB_ENABLED configuration already exists",
                "IS_GITEA_ENABLED configuration already exists",
            ]
        );
        assert!(!store.has_row("IS_GOOGLE_ENABLED"));
        assert!(!store.has_row("IS_GITHUB_ENABLED"));
        assert!(!store.has_row("IS_GITLAB_ENABLED"));
        assert_eq!(
            store
                .get_row("IS_GITEA_ENABLED")
                .expect("seeded")
                .value
                .as_deref(),
            Some("0")
        );
    }

    #[test]
    fn rerun_warns_and_preserves_existing_values() {
        let store = MemSeed::default();
        store.put_row(
            "ENABLE_SIGNUP",
            MemRow {
                value: Some("custom".to_string()),
                category: "AUTHENTICATION".to_string(),
                is_encrypted: false,
            },
        );
        let registry = registry();
        let env = MapEnv::default().with("SECRET_KEY", "s3cret");
        let report = block(configure_instance(&env, &store, &registry, &keyring()))
            .expect("configure succeeds");
        // The pre-existing row keeps its value; only the other 35 seed
        // (IS_GITEA_ENABLED seeds fresh here, so the derived gate trips).
        assert_eq!(
            store
                .get_row("ENABLE_SIGNUP")
                .expect("seeded")
                .value
                .as_deref(),
            Some("custom")
        );
        assert_eq!(report.seeded_keys().len(), 35);
        assert!(report.lines.iter().any(|line| {
            line.kind == LineKind::Warning
                && line.text == "ENABLE_SIGNUP configuration already exists"
        }));
    }

    #[test]
    fn encrypted_seed_round_trips_through_the_keyring() {
        let store = MemSeed::default();
        let registry = registry();
        let env = MapEnv::default()
            .with("SECRET_KEY", "s3cret")
            .with("GOOGLE_CLIENT_SECRET", "shh");
        block(configure_instance(&env, &store, &registry, &keyring())).expect("configure succeeds");
        let stored = store
            .get_row("GOOGLE_CLIENT_SECRET")
            .expect("seeded")
            .value
            .clone()
            .expect("seeded");
        assert!(
            store
                .get_row("GOOGLE_CLIENT_SECRET")
                .expect("seeded")
                .is_encrypted
        );
        assert_ne!(stored, "shh");
        assert_eq!(keyring().decrypt(&stored), "shh");
    }

    #[test]
    fn unset_encrypted_seed_stores_empty_string() {
        // `encrypt_data(None)` → `""` (`configure_instance.py:52-55`).
        let store = MemSeed::default();
        let registry = registry();
        let env = MapEnv::default().with("SECRET_KEY", "s3cret");
        block(configure_instance(&env, &store, &registry, &keyring())).expect("configure succeeds");
        assert_eq!(
            store
                .get_row("GOOGLE_CLIENT_SECRET")
                .expect("seeded")
                .value
                .as_deref(),
            Some("")
        );
        // Plain unset values stay NULL.
        assert_eq!(
            store.get_row("GOOGLE_CLIENT_ID").expect("seeded").value,
            None
        );
    }

    #[test]
    fn non_db_sourced_keys_are_skipped() {
        let store = MemSeed::default();
        // Cloud-style override: the key is env-sourced (SSM), never seeded.
        let registry = ConfigRegistry::build_with_overrides(HashMap::from([(
            "GOOGLE_CLIENT_ID".to_string(),
            ConfigSource::Env,
        )]));
        let env = MapEnv::default()
            .with("SECRET_KEY", "s3cret")
            .with("GOOGLE_CLIENT_ID", "from-env");
        block(configure_instance(&env, &store, &registry, &keyring())).expect("configure succeeds");
        assert!(!store.has_row("GOOGLE_CLIENT_ID"));
    }

    #[test]
    fn derived_flags_run_when_none_exists_with_caller_default_quirks() {
        // Skip the seed loop's IS_GITEA_ENABLED (env-sourced here) so the
        // any-exists gate opens and the derived computations run.
        let store = MemSeed::default();
        let registry = ConfigRegistry::build_with_overrides(HashMap::from([(
            "IS_GITEA_ENABLED".to_string(),
            ConfigSource::Env,
        )]));
        let env = MapEnv::default()
            .with("SECRET_KEY", "s3cret")
            .with("GOOGLE_CLIENT_ID", "id-only")
            .with("GOOGLE_CLIENT_SECRET", "s3cr3t");
        let report = block(configure_instance(&env, &store, &registry, &keyring()))
            .expect("configure succeeds");
        // 35 seeds (IS_GITEA_ENABLED skipped as env-sourced) + 4 derived
        // rows, all reported with the same "loaded with value" line.
        assert_eq!(report.seeded_keys().len(), 39);
        // Full presence → "1". (Note the ordering quirk this port keeps:
        // the seed loop runs first, so an UNSET secret is already seeded as
        // `""` and the `"0"` caller default never applies post-seed — with
        // only the client id set the flag would be `"0"`, exactly like
        // Python reading back its own seeded row.)
        assert_eq!(
            store
                .get_row("IS_GOOGLE_ENABLED")
                .expect("seeded")
                .value
                .as_deref(),
            Some("1")
        );
        assert_eq!(
            store.get_row("IS_GOOGLE_ENABLED").expect("seeded").category,
            "AUTHENTICATION"
        );
        // Nothing set: GITHUB secret `"0"` is truthy but the id is empty.
        assert_eq!(
            store
                .get_row("IS_GITHUB_ENABLED")
                .expect("seeded")
                .value
                .as_deref(),
            Some("0")
        );
        // `GITLAB_HOST` defaults truthy, but id + secret are empty.
        assert_eq!(
            store
                .get_row("IS_GITLAB_ENABLED")
                .expect("seeded")
                .value
                .as_deref(),
            Some("0")
        );
        assert_eq!(
            store
                .get_row("IS_GITEA_ENABLED")
                .expect("seeded")
                .value
                .as_deref(),
            Some("0")
        );
        assert!(report.lines.iter().any(|line| {
            line.kind == LineKind::Success
                && line.text == "IS_GOOGLE_ENABLED loaded with value from environment variable."
        }));
    }

    #[test]
    fn derived_flag_truth_tables_match_fixture() {
        let recorded = &fixture("configure_instance.golden.json")["derived_flag_logic"];
        assert!(recorded["IS_GOOGLE_ENABLED"]
            .as_str()
            .unwrap()
            .contains("'0'"));
        assert!(recorded["IS_GITLAB_ENABLED"]
            .as_str()
            .unwrap()
            .contains("https://gitlab.com"));
        // Full presence → "1" for every flag.
        let present = [
            ConfigValue::Str("host".to_string()),
            ConfigValue::Str("id".to_string()),
            ConfigValue::Str("secret".to_string()),
        ];
        assert!(present.iter().all(is_truthy));
        // `bool("")` and `bool(Null)` are falsy, like Python.
        assert!(!is_truthy(&ConfigValue::Str(String::new())));
        assert!(!is_truthy(&ConfigValue::Null));
    }

    // -- register_instance --------------------------------------------------

    #[derive(Default)]
    struct MemState {
        row: Option<StoredInstance>,
        creates: usize,
        updates: Vec<(uuid::Uuid, String, Option<String>, bool)>,
    }

    #[derive(Default)]
    struct MemInstances {
        state: std::sync::Mutex<MemState>,
    }

    impl MemInstances {
        fn with_row(instance_id: &str) -> Self {
            Self {
                state: std::sync::Mutex::new(MemState {
                    row: Some(StoredInstance {
                        id: uuid::Uuid::new_v4(),
                        instance_id: instance_id.to_string(),
                    }),
                    creates: 0,
                    updates: Vec::new(),
                }),
            }
        }

        fn creates(&self) -> usize {
            self.state.lock().expect("test lock").creates
        }

        fn updates(&self) -> Vec<(uuid::Uuid, String, Option<String>, bool)> {
            self.state.lock().expect("test lock").updates.clone()
        }

        fn has_row(&self) -> bool {
            self.state.lock().expect("test lock").row.is_some()
        }
    }

    impl InstanceStore for MemInstances {
        async fn first(&self) -> Result<Option<StoredInstance>, CommandError> {
            Ok(self.state.lock().expect("test lock").row.clone())
        }

        async fn create(&self, input: InstanceInput) -> Result<StoredInstance, CommandError> {
            let mut state = self.state.lock().expect("test lock");
            state.creates += 1;
            let row = StoredInstance {
                id: uuid::Uuid::new_v4(),
                instance_id: input.instance_id.clone(),
            };
            state.row = Some(row.clone());
            Ok(row)
        }

        async fn update_check(
            &self,
            id: uuid::Uuid,
            current_version: &str,
            latest_version: Option<&str>,
            is_test: bool,
            _checked_at: DateTime<Utc>,
        ) -> Result<(), CommandError> {
            self.state.lock().expect("test lock").updates.push((
                id,
                current_version.to_string(),
                latest_version.map(str::to_string),
                is_test,
            ));
            Ok(())
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("fixed test time")
    }

    #[test]
    fn create_branch_registers_then_enqueues_traces() {
        let store = MemInstances::default();
        let env = MapEnv::default()
            .with("APP_VERSION", "v9.9.9")
            .with("IS_TEST", "1");
        let report = block(register_instance(
            &env,
            &store,
            Some("sig"),
            &PackageJson::Parsed(Some("x".to_string())),
            LatestProbe::Tag(Some("v10.0.0".to_string())),
            now(),
        ))
        .expect("register succeeds");
        assert!(report.created);
        assert_eq!(report.current_version, "v9.9.9");
        assert_eq!(report.latest_version, "v10.0.0");
        assert_eq!(report.stdout, vec!["Instance registered".to_string()]);
        assert_eq!(report.instance_id.len(), 24);
        assert!(report.instance_id.chars().all(|c| c.is_ascii_hexdigit()));
        // The `instance_traces.delay()` tail, always enqueued (`:90`).
        assert_eq!(report.delay.task, crate::license::tasks::TASK_NAME);
        assert!(report.delay.args.is_empty());
        assert!(store.has_row());
    }

    #[test]
    fn update_branch_refreshes_check_fields_and_keeps_identity() {
        let store = MemInstances::with_row("existing-token");
        let env = MapEnv::default().with("APP_VERSION", "v9.9.9");
        let report = block(register_instance(
            &env,
            &store,
            Some("sig"),
            &PackageJson::Parsed(Some("x".to_string())),
            LatestProbe::Tag(Some("v10.0.0".to_string())),
            now(),
        ))
        .expect("register succeeds");
        assert!(!report.created);
        assert_eq!(report.instance_id, "existing-token");
        assert_eq!(
            report.stdout,
            vec!["Instance already registered".to_string()]
        );
        assert_eq!(store.creates(), 0);
        assert_eq!(store.updates().len(), 1);
        assert_eq!(store.updates()[0].1, "v9.9.9");
        assert_eq!(store.updates()[0].2.as_deref(), Some("v10.0.0"));
        assert!(!store.updates()[0].3);
        assert_eq!(report.delay.task, crate::license::tasks::TASK_NAME);
    }

    #[test]
    fn current_version_chain_env_then_package_then_default() {
        let mut stdout = Vec::new();
        // Env wins, even over the package file.
        assert_eq!(
            resolve_current_version(
                Some("v9.0.0"),
                &PackageJson::Parsed(Some("v1.0.0".to_string())),
                &mut stdout,
            ),
            "v9.0.0"
        );
        // Empty env counts as unset (Python falsiness, `:28`).
        assert_eq!(
            resolve_current_version(
                Some(""),
                &PackageJson::Parsed(Some("v1.0.0".to_string())),
                &mut stdout,
            ),
            "v1.0.0"
        );
        // Parsed file without a version key: silent fallback.
        assert_eq!(
            resolve_current_version(Some(""), &PackageJson::Parsed(None), &mut stdout),
            "v0.1.0"
        );
        assert!(stdout.is_empty());
        // Unreadable file: the error line, then the fallback.
        assert_eq!(
            resolve_current_version(Some(""), &PackageJson::Unreadable, &mut stdout),
            "v0.1.0"
        );
        assert_eq!(
            stdout,
            vec!["Error checking for current version".to_string()]
        );
    }

    #[test]
    fn latest_version_falls_back_to_current_with_error_line() {
        let mut stdout = Vec::new();
        assert_eq!(
            resolve_latest_version(
                LatestProbe::Tag(Some("v2.0.0".to_string())),
                "v1.0.0",
                &mut stdout,
            ),
            "v2.0.0"
        );
        // Missing tag key: silent fallback (`data.get("tag_name", …)`).
        assert_eq!(
            resolve_latest_version(LatestProbe::Tag(None), "v1.0.0", &mut stdout),
            "v1.0.0"
        );
        assert!(stdout.is_empty());
        assert_eq!(
            resolve_latest_version(LatestProbe::Failed, "v1.0.0", &mut stdout),
            "v1.0.0"
        );
        assert_eq!(
            stdout,
            vec!["Error checking for latest version".to_string()]
        );
    }

    #[test]
    fn machine_signature_empty_raises_with_default_for_missing() {
        let store = MemInstances::default();
        let env = MapEnv::default();
        assert_eq!(
            block(register_instance(
                &env,
                &store,
                Some(""),
                &PackageJson::Parsed(None),
                LatestProbe::Failed,
                now(),
            )),
            Err(CommandError::MachineSignature)
        );
        // Missing option defaults to "machine-signature" (`:62`).
        let store = MemInstances::default();
        block(register_instance(
            &env,
            &store,
            None,
            &PackageJson::Parsed(None),
            LatestProbe::Failed,
            now(),
        ))
        .expect("default signature registers");
        assert_eq!(store.creates(), 1);
    }

    #[test]
    fn is_test_flag_is_exact_string_match() {
        for (env_value, expected) in [
            (None, false),
            (Some("0"), false),
            (Some(""), false),
            (Some("1"), true),
        ] {
            let env_value: Option<&str> = env_value;
            let store = MemInstances::with_row("token");
            let mut env = MapEnv::default();
            if let Some(value) = env_value {
                env = env.with("IS_TEST", value);
            }
            block(register_instance(
                &env,
                &store,
                None,
                &PackageJson::Parsed(None),
                LatestProbe::Tag(None),
                now(),
            ))
            .expect("register succeeds");
            assert_eq!(store.updates()[0].3, expected, "IS_TEST={env_value:?}");
        }
    }

    #[test]
    fn generated_tokens_are_24_hex_chars_and_unique() {
        let first = generate_instance_token();
        let second = generate_instance_token();
        for token in [&first, &second] {
            assert_eq!(token.len(), 24);
            assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        }
        assert_ne!(first, second);
    }
}
