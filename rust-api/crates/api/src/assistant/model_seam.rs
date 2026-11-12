//! CE assistant model-provider seam (PIDASHCONV-914).
//!
//! Implements [`AssistantModelSeam`](pidash_services::extensions::AssistantModelSeam)
//! for the open-source build: BYOK only, with the title transport the
//! serving boundary owns. The serving state carries one of these by
//! default; the private overlay crate installs its own implementation
//! (OpenHub/BYOK/Codex routing) without touching any caller.
//!
//! The pure decisions (`has_usable_llm_config`, `agent_model_profile`,
//! `agent_model_credential`) are the trait defaults, which delegate to
//! the ported [`pidash_services::assistant::seams`] logic — there is
//! exactly one CE implementation of each. Only title generation is
//! spelled out here: it needs decrypt + provider HTTP, which live at
//! this layer (the `title` layering rule), so the trait declares it and
//! this struct runs the exact CE path the generate-title handler ran
//! before the seam existed (`llm_config.rs`, moved verbatim):
//! config-row gates, KMS/fernet decrypt, one provider call, the shared
//! response pipeline. `Ok("")` stays the caller's `generation_failed`
//! (the view's `if not title`); provider HTTP failures arrive as
//! [`AssistantError::ProviderUnreachable`], which renders byte-identical
//! to the view-inline 502.

use std::sync::Arc;

use serde_json::Value;

use pidash_db::config::Settings;
use pidash_services::assistant::llm as llm_service;
use pidash_services::assistant::title as title_service;
use pidash_services::extensions::{AssistantModelSeam, SeamFuture, TitleError, TitleRequest};
use pidash_types::assistant::errors::AssistantError;

/// CE assistant seam: BYOK only. Holds the resolved settings so title
/// decrypt reads the same effective crypto config as every handler.
#[derive(Debug, Clone)]
pub struct CeAssistantSeam {
    settings: Arc<Settings>,
}

impl CeAssistantSeam {
    pub fn new(settings: Arc<Settings>) -> Self {
        Self { settings }
    }
}

/// Map a decrypt failure the way the views' `try/except AssistantError`
/// does (the `crypto_failure` split): data problems render as the
/// wrapped assistant error, operational problems answer the generic 500.
fn map_crypto_error(err: pidash_services::assistant::crypto::CryptoError) -> TitleError {
    match err {
        pidash_services::assistant::crypto::CryptoError::NotConfigured(assistant) => {
            TitleError::Assistant(assistant)
        }
        pidash_services::assistant::crypto::CryptoError::Transport(_) => TitleError::ServerError,
    }
}

impl AssistantModelSeam for CeAssistantSeam {
    fn generate_title(&self, request: TitleRequest) -> SeamFuture<'_, Result<String, TitleError>> {
        Box::pin(async move {
            let Some(byok) = request.byok else {
                return Err(TitleError::Assistant(AssistantError::LlmConfigMissing(
                    llm_service::MSG_NO_PROVIDER.to_owned(),
                )));
            };
            // Same three gates as `resolve_byok_model`
            // (`runtime/llm.py:71-84`), re-checked at execution time so a
            // re-pointed base URL is still rejected before connecting.
            let model = match llm_service::resolve_byok_model(
                byok.has_api_key,
                &byok.model_name,
                &byok.provider_kind,
                &byok.base_url,
                byok.base_url_blocked,
            ) {
                Ok(model) => model,
                Err(err) => return Err(TitleError::Assistant(err)),
            };
            let config = super::common::crypto_config_from_settings(&self.settings);
            let api_key = match super::common::decrypt_api_key(
                &config,
                byok.api_key_encrypted.as_deref().unwrap_or(&[]),
            )
            .await
            {
                Ok(key) => key,
                Err(err) => return Err(map_crypto_error(err)),
            };
            match run_title_with_key(&model, &byok.base_url, &api_key, &request.description).await {
                TitleOutcome::Title(title) => Ok(title),
                TitleOutcome::ProviderFailed => {
                    Err(TitleError::Assistant(AssistantError::ProviderUnreachable(
                        "Could not reach the AI provider.".to_owned(),
                    )))
                }
                TitleOutcome::Empty => Ok(String::new()),
            }
        })
    }
}

/// Title outcome: the cleaned title, a provider failure (the generic
/// 502, `llm_config.py:146-150`), or an empty model reply (the
/// `generation_failed` 502, `:152-156`).
#[derive(Debug)]
enum TitleOutcome {
    Title(String),
    ProviderFailed,
    Empty,
}

async fn run_title_with_key(
    model: &llm_service::ModelRef,
    base_url: &str,
    api_key: &str,
    description: &str,
) -> TitleOutcome {
    match run_title(model, base_url, description, api_key).await {
        Some(title) if !title.is_empty() => TitleOutcome::Title(title),
        Some(_) => TitleOutcome::Empty,
        None => TitleOutcome::ProviderFailed,
    }
}

/// Run one title request against the resolved provider and return the
/// cleaned title, or `None` when the provider call fails at any point
/// (the title path collapses every provider failure into the 502 — no
/// classification like the test probe). Credentials travel like the
/// SDKs send them: `x-api-key` for Anthropic, Bearer [REDACTED]
/// OpenAI-compatible.
async fn run_title(
    model: &llm_service::ModelRef,
    base_url: &str,
    description: &str,
    api_key: &str,
) -> Option<String> {
    // Default redirect policy (follow): the ported SDK clients (`openai`,
    // `anthropic`) both default `follow_redirects` to true, unlike the
    // probe paths.
    let client = reqwest::Client::new();
    let timeout = std::time::Duration::from_secs_f64(title_service::TITLE_TIMEOUT_SECS);
    let response = match model {
        llm_service::ModelRef::Anthropic { model } => {
            let body = title_service::AnthropicTitleRequest { model, description }.body_json();
            client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .body(serde_json::to_string(&body).expect("title body serializes"))
                .timeout(timeout)
                .send()
                .await
        }
        llm_service::ModelRef::OpenAICompatible { model, .. } => {
            let body = title_service::OpenAiTitleRequest {
                model,
                base_url,
                description,
            }
            .body_json();
            client
                .post(format!(
                    "{}/chat/completions",
                    base_url.trim_end_matches('/')
                ))
                .bearer_auth(api_key)
                .header("Content-Type", "application/json")
                .body(serde_json::to_string(&body).expect("title body serializes"))
                .timeout(timeout)
                .send()
                .await
        }
    };
    let response = response.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let text = response.text().await.ok()?;
    let parsed: Value = serde_json::from_str(&text).ok()?;
    let content = match model {
        llm_service::ModelRef::Anthropic { .. } => parsed.get("content")?,
        llm_service::ModelRef::OpenAICompatible { .. } => parsed
            .get("choices")?
            .get(0)?
            .get("message")?
            .get("content")?,
    };
    Some(title_service::clean_title(&title_service::content_to_text(
        content,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_services::extensions::{ByokTitleInputs, CredentialError};

    fn seam() -> CeAssistantSeam {
        CeAssistantSeam::new(Arc::new(Settings::test_defaults()))
    }

    fn user() -> uuid::Uuid {
        uuid::Uuid::nil()
    }

    fn byok() -> ByokTitleInputs {
        ByokTitleInputs {
            model_name: "m".to_owned(),
            provider_kind: "openai_compatible".to_owned(),
            base_url: "https://8.8.8.8/v1".to_owned(),
            base_url_blocked: false,
            api_key_encrypted: Some(b"blob".to_vec()),
            has_api_key: true,
        }
    }

    fn request(byok: Option<ByokTitleInputs>) -> TitleRequest {
        TitleRequest {
            user_id: user(),
            description: "Fix the thing".to_owned(),
            byok,
        }
    }

    #[tokio::test]
    async fn ce_gate_matches_the_ported_seam() {
        let seam = seam();
        assert!(seam.has_usable_llm_config(user(), true).await);
        assert!(!seam.has_usable_llm_config(user(), false).await);
    }

    #[tokio::test]
    async fn ce_profile_matches_the_ported_matrix() {
        let seam = seam();
        let with_key = seam.agent_model_profile(user(), true).await;
        assert!(!with_key.available);
        assert_eq!(with_key.lane, "byok");
        assert_eq!(with_key.reason_code, "byok_not_supported_on_desktop");
        let without_key = seam.agent_model_profile(user(), false).await;
        assert!(!without_key.available);
        assert_eq!(without_key.reason_code, "llm_config_missing");
    }

    #[tokio::test]
    async fn ce_credential_matches_the_ported_refusal() {
        let err = seam()
            .agent_model_credential(user())
            .await
            .expect_err("CE has no desktop lane");
        let CredentialError::Unavailable(unavailable) = &err else {
            panic!("CE refuses via the profile arm, got {err:?}");
        };
        assert_eq!(unavailable.reason, "byok_not_supported_on_desktop");
        assert_eq!(
            unavailable.message,
            "This build has no model lane the desktop agent can use."
        );
    }

    #[tokio::test]
    async fn title_without_a_row_answers_llm_config_missing() {
        let err = seam()
            .generate_title(request(None))
            .await
            .expect_err("no row");
        assert_eq!(
            err,
            TitleError::Assistant(AssistantError::LlmConfigMissing(
                "No LLM provider is configured for this user.".to_owned()
            ))
        );
    }

    #[tokio::test]
    async fn title_gates_match_resolve_byok_model() {
        // Keyless row: the first gate, before any decrypt or HTTP.
        let keyless = ByokTitleInputs {
            has_api_key: false,
            ..byok()
        };
        let err = seam()
            .generate_title(request(Some(keyless)))
            .await
            .expect_err("keyless");
        let TitleError::Assistant(assistant) = &err else {
            panic!("keyless must render, got {err:?}");
        };
        assert_eq!(assistant.code(), "llm_config_missing");
        // Empty model name: the second gate.
        let modeless = ByokTitleInputs {
            model_name: String::new(),
            ..byok()
        };
        let err = seam()
            .generate_title(request(Some(modeless)))
            .await
            .expect_err("modeless");
        assert_eq!(
            err,
            TitleError::Assistant(AssistantError::LlmConfigMissing(
                "No model name is configured.".to_owned()
            ))
        );
        // Blocked base URL: the third gate.
        let blocked = ByokTitleInputs {
            base_url_blocked: true,
            ..byok()
        };
        let err = seam()
            .generate_title(request(Some(blocked)))
            .await
            .expect_err("blocked");
        assert_eq!(
            err,
            TitleError::Assistant(AssistantError::LlmConfigMissing(
                "The configured provider endpoint is not allowed.".to_owned()
            ))
        );
    }

    #[tokio::test]
    async fn title_decrypt_failure_maps_like_crypto_failure() {
        // Fernet backend, garbage ciphertext: a data problem, so the
        // wrapped assistant error renders (no network on this path).
        let mut settings = Settings::test_defaults();
        settings.assistant.crypto_backend = "fernet".to_owned();
        settings.assistant.encryption_key =
            "Lk0_5ciI2pVpujWpFeeysfbbGfRDTBMu-tNTJb7lLMw=".to_owned();
        let seam = CeAssistantSeam::new(Arc::new(settings));
        let err = seam
            .generate_title(request(Some(byok())))
            .await
            .expect_err("garbage ciphertext");
        let TitleError::Assistant(assistant) = &err else {
            panic!("bad ciphertext must render, got {err:?}");
        };
        assert_eq!(assistant.code(), "assistant_not_configured");
    }
}
