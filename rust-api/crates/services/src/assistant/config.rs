//! BYOK LLM config row service (OVL-07, PIDASHCONV-1545).
//!
//! The non-HTTP half of `assistant/views/llm_config.py:41-78` (the
//! `UserLLMConfigEndpoint` GET/PUT/DELETE), moved out of
//! `pidash_api::assistant::llm_config` so the cloud overlay's config
//! twin (`rust-cloud/.../assistant/config.rs`, which copied the row
//! read, the partial-update merge, the SSRF gate and the encrypt+save)
//! calls it instead.
//!
//! Moved verbatim: the row select (`models.py:238-262`), the
//! partial-update merge (omitted fields keep the stored value, create
//! falls back to the model default), the save-time SSRF gate
//! (`llm_config.py:53`), the encrypt-on-save (`:67`), and the
//! `cfg.save()` failure shape (400 `{"error": "invalid"}`, `:71-73`).
//! Writes go through [`crate::domain::assistant`]; the KMS transport
//! and the SSRF resolver arrive injected (the caller owns network
//! handles), and validation stays with the HTTP shell —
//! `put_config` takes the already-validated [`ValidatedPut`] that
//! `pidash_api::assistant::llm_config::validate_put` returns. (That
//! function moves here once OVL-04's compat crate lands: its field
//! mechanics are OVL-04's named unit.)

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::assistant::models::user_llm_config;

use crate::assistant::crypto::{self, CryptoConfig, CryptoError, KmsTransport};
use crate::assistant::ssrf::{self, HostResolver};
use crate::domain::assistant as domain;

/// One `assistant_user_llm_config` row as the config service reads it.
#[derive(Debug, Clone)]
pub struct LlmConfigRow {
    pub provider_kind: String,
    pub base_url: String,
    pub model_name: String,
    pub api_key_encrypted: Option<Vec<u8>>,
    pub last_verified_at: Option<DateTime<Utc>>,
}

/// One `assistant_user_llm_config` row, all 9 columns
/// (`models.py:238-262`): the narrow [`LlmConfigRow`] plus the pk, the
/// owner and the stamps. The overlay's connection twin
/// (`rust-cloud/.../assistant/connections.rs`, OVL-15) feeds this to
/// [`domain::save_llm_config_full`], which needs the id and the stamps.
#[derive(Debug, Clone)]
pub struct LlmConfigFullRow {
    pub id: i64,
    pub user_id: Uuid,
    pub provider_kind: String,
    pub base_url: String,
    pub model_name: String,
    pub api_key_encrypted: Option<Vec<u8>>,
    pub last_verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Validated PUT fields; `None` is "omitted" (partial-update keeps the
/// stored value, create falls back to the model default).
#[derive(Debug, Default)]
pub struct ValidatedPut {
    pub provider_kind: Option<String>,
    pub base_url: Option<String>,
    pub model_name: Option<String>,
    pub api_key: Option<String>,
}

/// What a config write can report, in handler-mapping order: the SSRF
/// refusal, a KMS failure, or the ported `cfg.save()` 400.
#[derive(Debug)]
pub enum ConfigError {
    /// `base_url_blocked` (`llm_config.py:53-61`).
    BaseUrlBlocked,
    /// `crypto.encrypt` failed (the services crypto wart:
    /// operational failures fold into the crypto error shape).
    Crypto(CryptoError),
    /// `cfg.save()` failed — even operationally — answering 400
    /// `{"error": "invalid"}` (`llm_config.py:71-73`).
    SaveFailed,
}

impl From<CryptoError> for ConfigError {
    fn from(error: CryptoError) -> Self {
        ConfigError::Crypto(error)
    }
}

/// The handles a config write needs: the pool, the crypto backend +
/// KMS transport, and the SSRF flag + resolver. The trait objects
/// carry `Send + Sync` so the service future stays `Send` for the
/// axum handler (the `LiveTurnSeam` resolver precedent).
pub struct ConfigDeps<'a> {
    pub pool: &'a PgPool,
    pub crypto: &'a CryptoConfig,
    pub kms: &'a (dyn KmsTransport + Send + Sync),
    pub block_private_urls: bool,
    pub ssrf_resolver: &'a (dyn HostResolver + Send + Sync),
}

/// Decoded full config-row tuple: id, owner, provider, base URL,
/// model, encrypted key, verification timestamp, stamps.
type LlmFullRowTuple = (
    i64,
    Uuid,
    String,
    String,
    String,
    Option<Vec<u8>>,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
    DateTime<Utc>,
);

/// `UserLLMConfig.objects.filter(user=...)` (`models.py:238-262`):
/// the caller's config row, all 9 columns, if any. Generic over pool
/// and connection: the overlay's select/adopt/get-or-create arms call
/// this on a `&mut PgConnection` inside a transaction (OVL-15,
/// PIDASHCONV-1651). `user_id` is unique (the `OneToOneField`), so at
/// most one row matches; the `LIMIT 1` is the moved `get_config`
/// predicate verbatim.
pub async fn fetch_llm_config_full<'e, E>(
    ex: E,
    user_id: Uuid,
) -> Result<Option<LlmConfigFullRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let row: Option<LlmFullRowTuple> = sqlx::query_as(
        r#"SELECT id, user_id, provider_kind, base_url, model_name, api_key_encrypted, last_verified_at, created_at, updated_at
           FROM assistant_user_llm_config WHERE user_id = $1 LIMIT 1"#,
    )
    .bind(user_id)
    .fetch_optional(ex)
    .await?;
    Ok(row.map(
        |(
            id,
            user_id,
            provider_kind,
            base_url,
            model_name,
            api_key_encrypted,
            last_verified_at,
            created_at,
            updated_at,
        )| LlmConfigFullRow {
            id,
            user_id,
            provider_kind,
            base_url,
            model_name,
            api_key_encrypted,
            last_verified_at,
            created_at,
            updated_at,
        },
    ))
}

/// `UserLLMConfig.objects.filter(user=...)` (`models.py:238-262`):
/// the caller's config row, if any. Thin caller of
/// [`fetch_llm_config_full`], projected to the narrow row the HTTP
/// shell renders.
pub async fn get_config(pool: &PgPool, user_id: Uuid) -> Result<Option<LlmConfigRow>, sqlx::Error> {
    Ok(fetch_llm_config_full(pool, user_id)
        .await?
        .map(|full| LlmConfigRow {
            provider_kind: full.provider_kind,
            base_url: full.base_url,
            model_name: full.model_name,
            api_key_encrypted: full.api_key_encrypted,
            last_verified_at: full.last_verified_at,
        }))
}

/// `PUT users/me/ai-assistant/config/` (`llm_config.py:46-74`): merge
/// the validated fields over the stored row, SSRF-gate the effective
/// base URL, encrypt a fresh key, and save. `existing` is the
/// pre-fetched row (the caller validates against it, so the PUT reads
/// once). Returns the saved row (`last_verified_at` keeps its stored
/// value — only the probe stamps it).
pub async fn put_config(
    deps: &ConfigDeps<'_>,
    user_id: Uuid,
    existing: Option<&LlmConfigRow>,
    validated: ValidatedPut,
) -> Result<LlmConfigRow, ConfigError> {
    let row = existing;
    let existed = row.is_some();
    let stored_provider = row.as_ref().map(|row| row.provider_kind.clone());
    let stored_base = row.as_ref().map(|row| row.base_url.clone());
    let stored_model = row.as_ref().map(|row| row.model_name.clone());
    let stored_verified = row.as_ref().and_then(|row| row.last_verified_at);
    let stored_key = row.as_ref().and_then(|row| row.api_key_encrypted.clone());

    let base_url_stored = validated
        .base_url
        .unwrap_or_else(|| stored_base.unwrap_or_default());
    if !base_url_stored.is_empty()
        && ssrf::is_blocked(
            &base_url_stored,
            deps.block_private_urls,
            deps.ssrf_resolver,
        )
    {
        return Err(ConfigError::BaseUrlBlocked);
    }

    let mut api_key_encrypted = stored_key;
    if let Some(api_key) = validated.api_key.filter(|key| !key.is_empty()) {
        api_key_encrypted = Some(crypto::encrypt(deps.crypto, &api_key, deps.kms)?);
    }
    let provider_kind = validated
        .provider_kind
        .or(stored_provider)
        .unwrap_or_else(|| user_llm_config::DEFAULT_PROVIDER_KIND.as_str().to_owned());
    let model_name = validated.model_name.or(stored_model).unwrap_or_default();
    domain::save_llm_config(
        deps.pool,
        &domain::LlmConfigSave {
            user_id,
            provider_kind: &provider_kind,
            base_url: &base_url_stored,
            model_name: &model_name,
            api_key_encrypted: &api_key_encrypted,
            existed,
        },
    )
    .await
    .map_err(|_| ConfigError::SaveFailed)?;
    Ok(LlmConfigRow {
        provider_kind,
        base_url: base_url_stored,
        model_name,
        api_key_encrypted,
        last_verified_at: stored_verified,
    })
}

/// `DELETE users/me/ai-assistant/config/` (`llm_config.py:76-78`).
pub async fn delete_config(pool: &PgPool, user_id: Uuid) -> Result<(), sqlx::Error> {
    domain::delete_llm_config(pool, user_id).await
}
