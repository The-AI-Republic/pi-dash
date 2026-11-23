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
use pidash_types::assistant::serializers;

/// One `assistant_user_llm_config` row as the config service reads it.
#[derive(Debug, Clone)]
pub struct LlmConfigRow {
    pub provider_kind: String,
    pub base_url: String,
    pub model_name: String,
    pub api_key_encrypted: Option<Vec<u8>>,
    pub last_verified_at: Option<DateTime<Utc>>,
    /// Every model ID held under this key (NEWPIDASH-392); the resolver
    /// matches saved per-surface choices against this list.
    pub models: Vec<String>,
    /// Model ID → ISO timestamp of the last passing test (NEWPIDASH-392).
    pub model_verified_at: serde_json::Value,
}

/// One `assistant_user_llm_config` row, all 11 columns
/// (`models.py:238-262` plus the NEWPIDASH-392 `models` /
/// `model_verified_at` pair): the narrow [`LlmConfigRow`] plus the pk,
/// the owner and the stamps. The overlay's connection twin
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
    /// Every model ID held under this key (NEWPIDASH-392); writers keep
    /// `model_name = models[0]`.
    pub models: Vec<String>,
    /// Model ID → ISO timestamp of the last passing test (NEWPIDASH-392).
    pub model_verified_at: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Validated PUT fields; `None` is "omitted" (partial-update keeps the
/// stored value, create falls back to the model default). `models` is
/// the raw list when the body names one (shape-checked by the HTTP
/// shell; list rules run in [`put_config`]); `save_unverified` is the
/// opt-in flag for saving provider-unreachable IDs.
#[derive(Debug, Default)]
pub struct ValidatedPut {
    pub provider_kind: Option<String>,
    pub base_url: Option<String>,
    pub model_name: Option<String>,
    pub api_key: Option<String>,
    pub models: Option<Vec<String>>,
    pub save_unverified: bool,
}

/// What a config write can report, in handler-mapping order: the SSRF
/// refusal, a KMS failure, the ported `cfg.save()` 400, the BYOK list
/// refusals, or the smoke-test rejection (nothing saved).
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
    /// An update to zero model IDs (`models_required`).
    ModelsRequired,
    /// A `models` list that fails the list rules (too long an ID, too
    /// many IDs): rendered as field errors on `models`.
    ModelsInvalid(String),
    /// The smoke test rejected the write (`model_test_failed` with the
    /// per-ID results); nothing was saved.
    ModelTestFailed(Vec<super::model_selection::ModelTestResult>),
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
/// model, encrypted key, verification timestamp, model IDs,
/// per-model verification map, stamps.
type LlmFullRowTuple = (
    i64,
    Uuid,
    String,
    String,
    String,
    Option<Vec<u8>>,
    Option<DateTime<Utc>>,
    Vec<String>,
    serde_json::Value,
    DateTime<Utc>,
    DateTime<Utc>,
);

/// SeaORM form of the config read: all 9 columns by `user_id` (`LIMIT 1`
/// semantics via `one()`), for pool- and transaction-held callers that can
/// hand a SeaORM connection (`sea_orm_connection` on a pool, `TxConn` on an
/// sqlx transaction). `get_config` below reads through this.
pub async fn fetch_llm_config_full_conn(
    db: &impl sea_orm::ConnectionTrait,
    user_id: Uuid,
) -> Result<Option<LlmConfigFullRow>, sqlx::Error> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let row = pidash_db::entity::assistant_user_llm_config::Entity::find()
        .filter(pidash_db::entity::assistant_user_llm_config::Column::UserId.eq(user_id))
        .one(db)
        .await
        .map_err(pidash_db::orm_tx::into_sqlx)?;
    Ok(row.map(|row| LlmConfigFullRow {
        id: row.id,
        user_id: row.user_id,
        provider_kind: row.provider_kind,
        base_url: row.base_url,
        model_name: row.model_name,
        api_key_encrypted: row.api_key_encrypted,
        last_verified_at: row.last_verified_at.map(|v| v.with_timezone(&Utc)),
        models: row.models,
        model_verified_at: row.model_verified_at,
        created_at: row.created_at.with_timezone(&Utc),
        updated_at: row.updated_at.with_timezone(&Utc),
    }))
}

/// `UserLLMConfig.objects.filter(user=...)` (`models.py:238-262`):
/// the caller's config row, all 9 columns, if any. Generic over pool
/// and connection: the overlay's select/adopt/get-or-create arms call
/// this on a `&mut PgConnection` inside a transaction (OVL-15,
/// PIDASHCONV-1651). `user_id` is unique (the `OneToOneField`), so at
/// most one row matches; the `LIMIT 1` is the moved `get_config`
/// predicate verbatim.
///
/// Kept as the generic `sqlx::Executor` entry point because the overlay's
/// transaction callers pass a bare `&mut PgConnection`; a SeaORM read needs
/// a `ConnectionTrait` wrapper (`TxConn`) at the call site, which would
/// change this shared helper's bound and those out-of-scope callers. Pool
/// readers should prefer [`fetch_llm_config_full_conn`] (and `get_config`
/// does). See the `// raw-sql:` note on the query below.
pub async fn fetch_llm_config_full<'e, E>(
    ex: E,
    user_id: Uuid,
) -> Result<Option<LlmConfigFullRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    // raw-sql: generic sqlx Executor contract shared with the overlay's &mut PgConnection callers; a SeaORM read would need a ConnectionTrait wrapper at every call site.
    let row: Option<LlmFullRowTuple> = sqlx::query_as(
        r#"SELECT id, user_id, provider_kind, base_url, model_name, api_key_encrypted, last_verified_at, models, model_verified_at, created_at, updated_at
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
            models,
            model_verified_at,
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
            models,
            model_verified_at,
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    Ok(fetch_llm_config_full_conn(&db, user_id)
        .await?
        .map(|full| LlmConfigRow {
            provider_kind: full.provider_kind,
            base_url: full.base_url,
            model_name: full.model_name,
            api_key_encrypted: full.api_key_encrypted,
            last_verified_at: full.last_verified_at,
            models: full.models,
            model_verified_at: full.model_verified_at,
        }))
}

/// `PUT users/me/ai-assistant/config/` (`llm_config.py:46-74`): merge
/// the validated fields over the stored row, SSRF-gate the effective
/// base URL, encrypt a fresh key, smoke-test the new model IDs and
/// save. `existing` is the pre-fetched row (the caller validates
/// against it, so the PUT reads once). Returns the per-ID smoke-test
/// results for the tested IDs, in order (empty when nothing was new).
/// A rejected write saves nothing (`ModelTestFailed`).
pub async fn put_config(
    deps: &ConfigDeps<'_>,
    user_id: Uuid,
    existing: Option<&LlmConfigRow>,
    validated: ValidatedPut,
) -> Result<Vec<super::model_selection::ModelTestResult>, ConfigError> {
    use super::model_selection::{self as selection, ModelTestResult, MAX_MODELS_PER_KEY};
    use super::probe;

    let row = existing;
    let existed = row.is_some();
    let stored_provider = row.as_ref().map(|row| row.provider_kind.clone());
    let stored_base = row.as_ref().map(|row| row.base_url.clone());
    let stored_model = row.as_ref().map(|row| row.model_name.clone());
    let stored_models = row
        .as_ref()
        .map(|row| row.models.clone())
        .unwrap_or_default();
    let mut stored_verified = row
        .as_ref()
        .and_then(|row| row.model_verified_at.as_object().cloned())
        .unwrap_or_default();
    let stored_key = row.as_ref().and_then(|row| row.api_key_encrypted.clone());

    // Effective scalar fields (partial-update merge, as before).
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
    let provider_kind = validated
        .provider_kind
        .or(stored_provider)
        .unwrap_or_else(|| user_llm_config::DEFAULT_PROVIDER_KIND.as_str().to_owned());

    // Effective model list (NEWPIDASH-392 §4.4, §6.3): a body `models`
    // list wins outright (`model_name`, when also sent, is ignored —
    // the list's first ID is the model); otherwise the single-model
    // sync keeps `model_name = models[0]` (§4.5).
    let from_body = validated.models.is_some();
    let mut models = match validated.models {
        Some(raw) => selection::validate_models_list(&raw).map_err(|error| {
            use super::model_selection::ModelsListError;
            match error {
                ModelsListError::Empty => ConfigError::ModelsRequired,
                ModelsListError::TooLong(id) => ConfigError::ModelsInvalid(format!(
                    "Model ID is too long (at most {} characters): {id}",
                    serializers::MODEL_NAME_MAX_LENGTH,
                )),
                ModelsListError::TooMany(count) => ConfigError::ModelsInvalid(format!(
                    "Too many model IDs (at most {MAX_MODELS_PER_KEY}): {count}",
                )),
            }
        })?,
        None => stored_models.clone(),
    };
    let model_name = if from_body {
        models[0].clone()
    } else {
        let model_name = validated.model_name.or(stored_model).unwrap_or_default();
        if !model_name.is_empty() && models.first().is_none_or(|first| first != &model_name) {
            models = vec![model_name.clone()];
        }
        model_name
    };

    // Effective key: fresh plaintext when the body names one, else the
    // decrypted stored key (absent when the user holds no key).
    let mut api_key_encrypted = stored_key;
    let mut plaintext_key: Option<String> = None;
    if let Some(api_key) = validated.api_key.filter(|key| !key.is_empty()) {
        api_key_encrypted = Some(crypto::encrypt(deps.crypto, &api_key, deps.kms)?);
        plaintext_key = Some(api_key);
    }
    let stored_plaintext = match api_key_encrypted.as_deref() {
        Some(bytes) if plaintext_key.is_none() => {
            Some(crypto::decrypt(deps.crypto, bytes, deps.kms)?)
        }
        _ => None,
    };
    let probe_key = plaintext_key.or(stored_plaintext).unwrap_or_default();

    // New IDs to smoke-test (NEWPIDASH-392 §7.2): a new key (all IDs),
    // a new ID on an existing key (that ID only), a changed key, base
    // URL or provider (all IDs). A create tests everything it saves.
    let lane_changed = provider_changed(row, &provider_kind)
        || base_changed(row, &base_url_stored)
        || key_material_changed(&validated_key_presence(row), &api_key_encrypted);
    let new_ids: Vec<String> = if !existed || lane_changed {
        models.clone()
    } else {
        models
            .iter()
            .filter(|model| !stored_models.iter().any(|stored| stored == *model))
            .cloned()
            .collect()
    };

    // Run the smoke test, then decide (nothing is saved on rejection).
    let mut results: Vec<ModelTestResult> = Vec::with_capacity(new_ids.len());
    if !new_ids.is_empty() {
        let timeout = std::time::Duration::from_secs(probe::SAVE_TEST_TIMEOUT_SECS);
        for (model, outcome) in probe::probe_models(
            &provider_kind,
            &base_url_stored,
            &new_ids,
            &probe_key,
            timeout,
        )
        .await
        {
            results.push(selection::classify_test(&model, &outcome));
        }
        if selection::smoke_test_blocks(&results, validated.save_unverified) {
            return Err(ConfigError::ModelTestFailed(results));
        }
    }

    // Merge the verification map and keep the legacy stamp on
    // `models[0]` (§4.5).
    let now = chrono::Utc::now();
    stored_verified = selection::merge_verified_map(stored_verified, &results, &now);
    let last_verified_at = models
        .first()
        .and_then(|first| stored_verified.get(first))
        .and_then(|value| value.as_str())
        .and_then(|text| text.parse::<chrono::DateTime<chrono::Utc>>().ok());
    let verified_value = serde_json::Value::Object(stored_verified);
    domain::save_llm_config(
        &pidash_db::pool::sea_orm_connection(deps.pool),
        &domain::LlmConfigSave {
            user_id,
            provider_kind: &provider_kind,
            base_url: &base_url_stored,
            model_name: &model_name,
            api_key_encrypted: &api_key_encrypted,
            models: &models,
            model_verified_at: &verified_value,
            last_verified_at,
            existed,
        },
    )
    .await
    .map_err(|_| ConfigError::SaveFailed)?;
    Ok(results)
}

/// True when the provider lane differs from the stored row's.
fn provider_changed(row: Option<&LlmConfigRow>, provider_kind: &str) -> bool {
    row.map(|row| row.provider_kind.as_str() != provider_kind)
        .unwrap_or(false)
}

/// True when the base URL differs from the stored row's.
fn base_changed(row: Option<&LlmConfigRow>, base_url: &str) -> bool {
    row.map(|row| row.base_url.as_str() != base_url)
        .unwrap_or(false)
}

/// True when key presence differs from the stored row's.
fn validated_key_presence(row: Option<&LlmConfigRow>) -> bool {
    row.and_then(|row| row.api_key_encrypted.as_ref())
        .is_some_and(|key| !key.is_empty())
}

/// True when the effective encrypted key differs in presence from the
/// stored row's (a fresh key was provided, or presence flipped).
fn key_material_changed(stored_has_key: &bool, effective: &Option<Vec<u8>>) -> bool {
    let effective_has_key = effective.as_ref().is_some_and(|key| !key.is_empty());
    *stored_has_key != effective_has_key
}

/// `DELETE users/me/ai-assistant/config/` (`llm_config.py:76-78`).
pub async fn delete_config(pool: &PgPool, user_id: Uuid) -> Result<(), sqlx::Error> {
    domain::delete_llm_config(&pidash_db::pool::sea_orm_connection(pool), user_id).await
}

#[cfg(test)]
mod tests_assistant_orm {
    #[test]
    fn fetch_config_seaorm_shape_pins_user_filter() {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryTrait};
        let sql = pidash_db::entity::assistant_user_llm_config::Entity::find()
            .filter(
                pidash_db::entity::assistant_user_llm_config::Column::UserId.eq(uuid::Uuid::nil()),
            )
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("assistant_user_llm_config"), "table: {sql}");
        assert!(sql.contains("user_id"), "filter: {sql}");
    }
}
