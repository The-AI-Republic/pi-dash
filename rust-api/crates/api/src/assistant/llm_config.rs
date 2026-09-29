#![forbid(unsafe_code)]

//! BYOK LLM config + title-generation handlers (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/views/llm_config.py:1-185`:
//!
//! * `UserLLMConfigEndpoint` (`:41-78`) — GET/PUT/DELETE on
//!   `users/me/ai-assistant/config/` with `_serialize` (`:23-38`).
//! * `UserLLMConfigTestEndpoint` (`:85-108`) — POST on
//!   `users/me/ai-assistant/config/test/` with `_run_test` (`:160-185`).
//! * `AssistantGenerateTitleEndpoint` (`:115-157`) — POST on
//!   `workspaces/<slug>/ai-assistant/generate-title/`, wired to the
//!   ported title pipeline
//!   ([`pidash_services::assistant::title`], `runtime/title.py:1-147`).
//!
//! Fixture ids: F-A6-01 (config shapes), F-A6-04 (key encrypt on save),
//! F-A6-05 (base_url SSRF check on save), F-A6-07 (test/title throttles),
//! F-A6-08 (title path).
//!
//! Layering: field validation reuses
//! [`pidash_types::assistant::serializers`], the provider gates reuse
//! [`pidash_services::assistant::llm::resolve_byok_model`], the title
//! request bodies and response pipeline reuse
//! [`pidash_services::assistant::title`], and the member gate reuses
//! [`super::perm::require_member`]. This module owns the HTTP shell
//! (routes, session auth), the config-row SQL, the DRF error rendering,
//! and the provider HTTP calls (the `pydantic-ai`/`openai`/`anthropic`
//! SDK clients stay in Python; the wire behavior is what is ported).
//!
//! Throttle scopes (`assistant_llm_test` 6/minute,
//! `assistant_llm_generate_title` 20/minute,
//! [`super::throttles::LLM_TEST_THROTTLE`]/[`GENERATE_TITLE_THROTTLE`])
//! are recorded but not enforced here: no merged Rust handler enforces
//! DRF throttles yet and no contract test pins a 429 shape (the harness
//! retries 429s), so enforcement follows the same precedent as every
//! other ported surface. DRF checks throttles before the handler body,
//! hence before the member gate.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * `cfg.save()` failures — even operational ones — answer 400
//!   `{"error": "invalid"}` (`llm_config.py:71-73`).
//! * KMS `encrypt` folds operational failures into `NotConfigured`
//!   (services crypto wart, `crypto.py:103-106`): a down KMS answers the
//!   crypto error shape, not the generic 500.
//! * A non-object POST body on `generate-title/` answers the generic 500
//!   (`request.data.get` raising `AttributeError`, no `except`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::Value;

use pidash_db::assistant::models::{has_secret, user_llm_config};
use pidash_services::assistant::llm as llm_service;
use pidash_services::assistant::title as title_service;
use pidash_types::assistant::errors::AssistantError;
use pidash_types::assistant::serializers;

use super::common::{
    actor, base_url_blocked, body_get, crypto_config, decrypt_api_key, encrypt_api_key, isoformat,
    ok_false, ok_false_detail, ok_true, parse_body, parse_put_body, pool_of, request_content_type,
    run_char_field, validate_choice, workspace_role, BodyField, BodyMap, CharFieldRules, Failure,
    ParsedBody,
};
use crate::state::AppState;

/// `assistant/urls.py:64` (under the `api/` include).
pub const CONFIG_PATH: &str = "/api/users/me/ai-assistant/config/";
/// `assistant/urls.py:66-69`.
pub const CONFIG_TEST_PATH: &str = "/api/users/me/ai-assistant/config/test/";
/// `assistant/urls.py:59-63` (under the `api/` include).
pub const GENERATE_TITLE_PATH: &str = "/api/workspaces/{slug}/ai-assistant/generate-title/";

/// Owned LLM-config routes. Sibling paths (`stt-config/`,
/// `transcribe/`, threads, messages, MCP, agent-profile) stay unmatched
/// and proxy to Django through the fallback; every unowned method on the
/// owned paths also proxies so Django answers its own 405-after-auth and
/// metadata OPTIONS byte for byte (the loop-handlers precedent).
pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route(
            CONFIG_PATH,
            owned_config(get(get_config).put(put_config).delete(delete_config)),
        )
        .route(CONFIG_TEST_PATH, owned_test(post(post_test)))
        .route(GENERATE_TITLE_PATH, owned_title(post(post_generate_title)))
}

/// The config path: GET + PUT + DELETE serve from Rust, everything else
/// falls through to Django.
fn owned_config(
    owned: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned
        .post(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .head(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// The test path: only POST exists in Django, so only POST is owned.
fn owned_test(
    owned: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned
        .get(crate::edge::proxy)
        .head(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// The title path: only POST exists in Django, so only POST is owned.
fn owned_title(
    owned: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned
        .get(crate::edge::proxy)
        .head(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

// ---------------------------------------------------------------------------
// config rows (`UserLLMConfig.objects.filter(user=...)`, `models.py:238-262`)
// ---------------------------------------------------------------------------

/// One `assistant_user_llm_config` row as the handlers read it.
#[derive(Debug, Clone)]
struct LlmRow {
    provider_kind: String,
    base_url: String,
    model_name: String,
    api_key_encrypted: Option<Vec<u8>>,
    last_verified_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Decoded config-row tuple: provider, base URL, model, encrypted key,
/// verification timestamp.
type LlmRowTuple = (
    String,
    String,
    String,
    Option<Vec<u8>>,
    Option<chrono::DateTime<chrono::Utc>>,
);

async fn fetch_row(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<Option<LlmRow>, Failure> {
    let row: Option<LlmRowTuple> = sqlx::query_as(
        r#"SELECT provider_kind, base_url, model_name, api_key_encrypted, last_verified_at
           FROM assistant_user_llm_config WHERE user_id = $1 LIMIT 1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Failure::server_error())?;
    Ok(row.map(
        |(provider_kind, base_url, model_name, api_key_encrypted, last_verified_at)| LlmRow {
            provider_kind,
            base_url,
            model_name,
            api_key_encrypted,
            last_verified_at,
        },
    ))
}

/// `cfg.save()` (`llm_config.py:71`): a full-row write that refreshes
/// `updated_at` (`auto_now`); `created_at`, `user_id`, and
/// `last_verified_at` keep their stored values. Any failure answers 400
/// `{"error": "invalid"}` (`:72-73`).
async fn save_row(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
    provider_kind: &str,
    base_url: &str,
    model_name: &str,
    api_key_encrypted: &Option<Vec<u8>>,
    existed: bool,
) -> Result<(), Failure> {
    let now = chrono::Utc::now();
    let result = if existed {
        sqlx::query(
            r#"UPDATE assistant_user_llm_config
               SET provider_kind = $1, base_url = $2, model_name = $3,
                   api_key_encrypted = $4, updated_at = $5
               WHERE user_id = $6"#,
        )
        .bind(provider_kind)
        .bind(base_url)
        .bind(model_name)
        .bind(api_key_encrypted)
        .bind(now)
        .bind(user_id)
        .execute(pool)
        .await
    } else {
        sqlx::query(
            r#"INSERT INTO assistant_user_llm_config
               (user_id, provider_kind, base_url, model_name, api_key_encrypted,
                last_verified_at, created_at, updated_at)
               VALUES ($1, $2, $3, $4, $5, NULL, $6, $7)"#,
        )
        .bind(user_id)
        .bind(provider_kind)
        .bind(base_url)
        .bind(model_name)
        .bind(api_key_encrypted)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
    };
    result
        .map(|_| ())
        .map_err(|_| Failure::bare_error(StatusCode::BAD_REQUEST, "invalid"))?;
    Ok(())
}

/// Render `_serialize` (`llm_config.py:23-38`): `Meta.fields` order minus
/// the write-only `api_key`; `last_verified_at` is `isoformat()` or
/// `None`. The unset shape carries the model default provider kind.
fn serialize_row(row: Option<&LlmRow>) -> Value {
    match row {
        None => serde_json::json!({
            "provider_kind": user_llm_config::DEFAULT_PROVIDER_KIND.as_str(),
            "base_url": "",
            "model_name": "",
            "has_api_key": false,
            "last_verified_at": null,
        }),
        Some(row) => serde_json::json!({
            "provider_kind": row.provider_kind,
            "base_url": row.base_url,
            "model_name": row.model_name,
            "has_api_key": has_secret(&row.api_key_encrypted),
            "last_verified_at": row.last_verified_at.as_ref().map(isoformat),
        }),
    }
}

// ---------------------------------------------------------------------------
// PUT validation (`UserLLMConfigSerializer`, `serializers.py:37-75`)
// ---------------------------------------------------------------------------

/// Validated PUT fields; `None` is "omitted" (partial-update keeps the
/// stored value, create falls back to the model default).
#[derive(Debug, Default)]
struct ValidatedPut {
    provider_kind: Option<String>,
    base_url: Option<String>,
    model_name: Option<String>,
    api_key: Option<String>,
}

/// Field-level + cross-field validation in DRF order: every present
/// writable field coerces, checks `max_length`, then runs its custom
/// `validate_<field>`; errors collect in `Meta.fields` order and the
/// cross-field `validate()` runs only when no field failed.
fn validate_put(
    body: &BodyMap,
    instance: Option<&LlmRow>,
) -> Result<ValidatedPut, Vec<(String, Vec<String>)>> {
    let mut errors: Vec<(String, Vec<String>)> = Vec::new();
    let mut out = ValidatedPut::default();

    if let Some(field) = body_get(body, "provider_kind") {
        match field {
            // `str(UploadedFile)` is the filename (verified live).
            BodyField::File { filename } => errors.push((
                "provider_kind".to_owned(),
                vec![format!("\"{filename}\" is not a valid choice.")],
            )),
            BodyField::Json(value) => match validate_choice(
                value,
                &[
                    serializers::PROVIDER_OPENAI_COMPATIBLE,
                    serializers::PROVIDER_ANTHROPIC,
                ],
            ) {
                Ok(kind) => out.provider_kind = Some(kind),
                Err(message) => errors.push(("provider_kind".to_owned(), vec![message])),
            },
        }
    }
    if let Some(field) = body_get(body, "base_url") {
        match field {
            BodyField::File { .. } => errors.push((
                "base_url".to_owned(),
                vec!["Not a valid string.".to_owned()],
            )),
            BodyField::Json(value) => match run_char_field(
                value,
                &CharFieldRules {
                    max_length: serializers::BASE_URL_MAX_LENGTH,
                    url: true,
                },
                serializers::validate_base_url,
            ) {
                Ok(normalized) => out.base_url = Some(normalized),
                Err(message) => errors.push(("base_url".to_owned(), vec![message])),
            },
        }
    }
    if let Some(field) = body_get(body, "model_name") {
        match field {
            BodyField::File { .. } => errors.push((
                "model_name".to_owned(),
                vec!["Not a valid string.".to_owned()],
            )),
            BodyField::Json(value) => match run_char_field(
                value,
                &CharFieldRules {
                    max_length: serializers::MODEL_NAME_MAX_LENGTH,
                    url: false,
                },
                serializers::validate_model_name,
            ) {
                Ok(normalized) => out.model_name = Some(normalized),
                Err(message) => errors.push(("model_name".to_owned(), vec![message])),
            },
        }
    }
    if let Some(field) = body_get(body, "api_key") {
        match field {
            BodyField::File { .. } => {
                errors.push(("api_key".to_owned(), vec!["Not a valid string.".to_owned()]))
            }
            BodyField::Json(value) => match run_char_field(
                value,
                &CharFieldRules {
                    max_length: serializers::API_KEY_MAX_LENGTH,
                    url: false,
                },
                serializers::validate_api_key,
            ) {
                Ok(key) => out.api_key = Some(key),
                Err(message) => errors.push(("api_key".to_owned(), vec![message])),
            },
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let attrs = serializers::LlmConfigAttrs {
        provider_kind: out.provider_kind.as_deref(),
        base_url: out.base_url.as_deref(),
    };
    let instance_view = instance.map(|row| serializers::LlmConfigInstance {
        provider_kind: row.provider_kind.as_str(),
        base_url: row.base_url.as_str(),
    });
    if let Err((field, message)) = serializers::validate_llm_config(&attrs, instance_view.as_ref())
    {
        errors.push((field.to_owned(), vec![message.to_owned()]));
        return Err(errors);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// handlers
// ---------------------------------------------------------------------------

/// `GET users/me/ai-assistant/config/` (`llm_config.py:42-44`).
async fn get_config(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(failure) => return failure.into_response(),
    };
    let actor = match actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(failure) => return failure.into_response(),
    };
    let row = match fetch_row(&pool, &actor.id).await {
        Ok(row) => row,
        Err(failure) => return failure.into_response(),
    };
    crate::license::json_response(&serialize_row(row.as_ref()))
}

/// `PUT users/me/ai-assistant/config/` (`llm_config.py:46-74`).
async fn put_config(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(failure) => return failure.into_response(),
    };
    let actor = match actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(failure) => return failure.into_response(),
    };
    let data = match parse_put_body(&body, request_content_type(&headers)) {
        Ok(data) => data,
        Err(failure) => return failure.into_response(),
    };
    let row = match fetch_row(&pool, &actor.id).await {
        Ok(row) => row,
        Err(failure) => return failure.into_response(),
    };
    let validated = match validate_put(&data, row.as_ref()) {
        Ok(validated) => validated,
        Err(errors) => return Failure::field_errors(errors).into_response(),
    };

    let existed = row.is_some();
    let stored_provider = row.as_ref().map(|row| row.provider_kind.clone());
    let stored_base = row.as_ref().map(|row| row.base_url.clone());
    let stored_model = row.as_ref().map(|row| row.model_name.clone());
    let stored_verified = row.as_ref().and_then(|row| row.last_verified_at);
    let stored_key = row.as_ref().and_then(|row| row.api_key_encrypted.clone());

    let base_url_stored = validated
        .base_url
        .unwrap_or_else(|| stored_base.unwrap_or_default());
    if base_url_blocked(&state, &base_url_stored) {
        return Failure::error_body(
            StatusCode::BAD_REQUEST,
            "base_url_blocked",
            "That endpoint host is not allowed.",
        )
        .into_response();
    }

    let mut api_key_encrypted = stored_key;
    if let Some(api_key) = validated.api_key.filter(|key| !key.is_empty()) {
        let config = crypto_config(&state);
        match encrypt_api_key(&config, &api_key).await {
            Ok(ciphertext) => api_key_encrypted = Some(ciphertext),
            Err(err) => {
                return super::common::crypto_failure(err, |assistant| {
                    Failure::assistant_error(assistant).into_response()
                });
            }
        }
    }
    let provider_kind = validated
        .provider_kind
        .or(stored_provider)
        .unwrap_or_else(|| user_llm_config::DEFAULT_PROVIDER_KIND.as_str().to_owned());
    let model_name = validated.model_name.or(stored_model).unwrap_or_default();
    if let Err(failure) = save_row(
        &pool,
        &actor.id,
        &provider_kind,
        &base_url_stored,
        &model_name,
        &api_key_encrypted,
        existed,
    )
    .await
    {
        return failure.into_response();
    }
    let saved = LlmRow {
        provider_kind,
        base_url: base_url_stored,
        model_name,
        api_key_encrypted,
        last_verified_at: stored_verified,
    };
    crate::license::json_response(&serialize_row(Some(&saved)))
}

/// `DELETE users/me/ai-assistant/config/` (`llm_config.py:76-78`).
async fn delete_config(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(failure) => return failure.into_response(),
    };
    let actor = match actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(failure) => return failure.into_response(),
    };
    if let Err(failure) = sqlx::query(r#"DELETE FROM assistant_user_llm_config WHERE user_id = $1"#)
        .bind(actor.id)
        .execute(&pool)
        .await
        .map(|_| ())
        .map_err(|_| Failure::server_error())
    {
        return failure.into_response();
    }
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::empty())
        .expect("delete response")
}

// ---------------------------------------------------------------------------
// connection test (`UserLLMConfigTestEndpoint`, `llm_config.py:85-108`)
// ---------------------------------------------------------------------------

/// Outcome of the provider probe: success, a classified failure with its
/// `(code, detail)`, or an unexpected transport failure (the outer
/// `except Exception`, `llm_config.py:101-102`).
#[derive(Debug, PartialEq)]
enum ProbeOutcome {
    Ok,
    Classified(&'static str, &'static str),
    TransportFailure,
}

/// Classify a probe HTTP round-trip the way `_run_test`
/// (`llm_config.py:160-185`) classifies the SDK exception text: 2xx is
/// success; otherwise the lowercased `"<status> <body>"` is scanned with
/// the same three keyword groups in the same order, defaulting to
/// `provider_unreachable`.
fn classify_probe(status: u16, body: &str) -> ProbeOutcome {
    if (200..300).contains(&status) {
        return ProbeOutcome::Ok;
    }
    let text = format!("{status} {}", body.to_lowercase());
    let has = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));
    if has(&["401", "unauthorized", "api key", "authentication"]) {
        ProbeOutcome::Classified("provider_auth_failed", "API key rejected.")
    } else if has(&["connection", "timeout", "unreachable", "resolve"]) {
        ProbeOutcome::Classified("provider_unreachable", "Could not reach the endpoint.")
    } else if has(&["model", "not found", "does not exist"]) {
        ProbeOutcome::Classified("model_invalid", "Model not accepted by the provider.")
    } else {
        ProbeOutcome::Classified("provider_unreachable", "Could not reach the endpoint.")
    }
}

/// Probe the configured provider with a single minimal prompt
/// (`agent.run("Reply with the single word: ok", request_limit=1)`,
/// `llm_config.py:168-172`). The Anthropic branch ignores `base_url`
/// exactly like `build_model` (`runtime/llm.py:99-103`); everything else
/// posts to `{base_url}/chat/completions`. Only the response class and
/// the keyword scan above are observed — bodies are never surfaced.
async fn probe_provider(
    provider_kind: &str,
    base_url: &str,
    model_name: &str,
    api_key: &str,
) -> ProbeOutcome {
    // No redirect following: pydantic-ai builds its SDK clients over its
    // own non-following httpx client (`create_async_http_client`), so a
    // 302 is observed as-is by `_run_test`, not chased.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("probe client builds");
    let result = if provider_kind == serializers::PROVIDER_ANTHROPIC {
        client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .body(
                serde_json::to_string(&serde_json::json!({
                    "model": model_name,
                    "messages": [{"role": "user", "content": "Reply with the single word: ok"}],
                    "max_tokens": 16,
                }))
                .expect("probe body serializes"),
            )
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
    } else {
        client
            .post(format!(
                "{}/chat/completions",
                base_url.trim_end_matches('/')
            ))
            .bearer_auth(api_key)
            .header("Content-Type", "application/json")
            .body(
                serde_json::to_string(&serde_json::json!({
                    "model": model_name,
                    "messages": [{"role": "user", "content": "Reply with the single word: ok"}],
                }))
                .expect("probe body serializes"),
            )
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
    };
    match result {
        Err(_) => ProbeOutcome::TransportFailure,
        Ok(response) => {
            let status = response.status().as_u16();
            match response.text().await {
                Ok(body) => classify_probe(status, &body),
                Err(_) => ProbeOutcome::TransportFailure,
            }
        }
    }
}

/// `POST users/me/ai-assistant/config/test/` (`llm_config.py:88-108`).
async fn post_test(
    State(state): State<AppState>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(failure) => return failure.into_response(),
    };
    let actor = match actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(failure) => return failure.into_response(),
    };
    let row = match fetch_row(&pool, &actor.id).await {
        Ok(row) => row,
        Err(failure) => return failure.into_response(),
    };
    let Some(row) = row.filter(|row| has_secret(&row.api_key_encrypted)) else {
        return ok_false("llm_config_missing");
    };
    if base_url_blocked(&state, &row.base_url) {
        return ok_false("base_url_blocked");
    }
    let config = crypto_config(&state);
    let api_key =
        match decrypt_api_key(&config, row.api_key_encrypted.as_deref().unwrap_or(&[])).await {
            Ok(key) => key,
            Err(err) => {
                return super::common::crypto_failure(err, |assistant| ok_false(assistant.code()));
            }
        };
    match probe_provider(&row.provider_kind, &row.base_url, &row.model_name, &api_key).await {
        ProbeOutcome::TransportFailure => ok_false("provider_unreachable"),
        ProbeOutcome::Classified(code, detail) => ok_false_detail(code, detail),
        ProbeOutcome::Ok => {
            let now = chrono::Utc::now();
            if let Err(failure) = sqlx::query(
                r#"UPDATE assistant_user_llm_config SET last_verified_at = $1 WHERE user_id = $2"#,
            )
            .bind(now)
            .bind(actor.id)
            .execute(&pool)
            .await
            .map(|_| ())
            .map_err(|_| Failure::server_error())
            {
                return failure.into_response();
            }
            ok_true()
        }
    }
}

// ---------------------------------------------------------------------------
// title generation (`AssistantGenerateTitleEndpoint`, `llm_config.py:115-157`)
// ---------------------------------------------------------------------------

/// Python `str()` over the `description` input
/// (`str(request.data.get("description") or "")`, `llm_config.py:135`):
/// falsy values (null, `""`, `0`, `false`, `[]`, `{}`) become `""`;
/// anything else stringifies (`True` → `"True"`, numbers as-is).
fn py_description(value: &Value) -> String {
    let falsy = match value {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Number(n) => {
            n.as_i64().is_some_and(|i| i == 0)
                || n.as_u64().is_some_and(|u| u == 0)
                || n.as_f64().is_some_and(|f| f == 0.0)
        }
        Value::String(s) => s.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
    };
    if falsy {
        return String::new();
    }
    super::common::py_scalar_display(value)
}

/// `POST workspaces/<slug>/ai-assistant/generate-title/`
/// (`llm_config.py:128-157`).
async fn post_generate_title(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(failure) => return failure.into_response(),
    };
    let actor = match actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(failure) => return failure.into_response(),
    };
    let role = match workspace_role(&pool, &actor.id, &slug).await {
        Ok(role) => role,
        Err(failure) => return failure.into_response(),
    };
    if super::perm::require_member(role).is_err() {
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(super::perm::ROLE_NOT_ALLOWED_BODY))
            .expect("role-denied response");
    }

    let data = match parse_body(&body, request_content_type(&headers)) {
        Ok(ParsedBody::Object(data)) => data,
        // A non-object body has no `.get`: `AttributeError` → generic
        // 500. (JSON `null` takes the same path: `None.get` raises too.)
        Ok(_) => return Failure::server_error().into_response(),
        Err(failure) => return failure.into_response(),
    };
    // Coerce to str before `.strip()`: an untrusted payload may send a
    // non-string description, which would otherwise 500
    // (`llm_config.py:133-134`). An uploaded file stringifies to its
    // filename, like `str()` of the Django value.
    let description = match body_get(&data, "description") {
        None => String::new(),
        Some(BodyField::Json(value)) => py_description(value),
        Some(BodyField::File { filename }) => filename.clone(),
    }
    .trim()
    .to_owned();
    if description.is_empty() {
        return Failure::error_body(
            StatusCode::BAD_REQUEST,
            "description_required",
            "A description is required to generate a title.",
        )
        .into_response();
    }

    let row = match fetch_row(&pool, &actor.id).await {
        Ok(row) => row,
        Err(failure) => return failure.into_response(),
    };
    let (row, has_key) = match row {
        Some(row) => {
            let has_key = has_secret(&row.api_key_encrypted);
            (row, has_key)
        }
        None => {
            return Failure::assistant_error(&AssistantError::LlmConfigMissing(
                llm_service::MSG_NO_PROVIDER.to_owned(),
            ))
            .into_response();
        }
    };
    // Same three gates as `resolve_byok_model` (`runtime/llm.py:71-84`),
    // re-checked at execution time so a re-pointed base URL is still
    // rejected before connecting.
    let blocked = base_url_blocked(&state, &row.base_url);
    let model = match llm_service::resolve_byok_model(
        has_key,
        &row.model_name,
        &row.provider_kind,
        &row.base_url,
        blocked,
    ) {
        Ok(model) => model,
        Err(err) => return Failure::assistant_error(&err).into_response(),
    };
    let config = crypto_config(&state);
    let api_key =
        match decrypt_api_key(&config, row.api_key_encrypted.as_deref().unwrap_or(&[])).await {
            Ok(key) => key,
            Err(err) => {
                return super::common::crypto_failure(err, |assistant| {
                    Failure::assistant_error(assistant).into_response()
                });
            }
        };
    match run_title_with_key(&model, &row, &api_key, &description).await {
        TitleOutcome::Title(title) => {
            crate::license::json_response(&serde_json::json!({"title": title}))
        }
        TitleOutcome::ProviderFailed => Failure::error_body(
            StatusCode::BAD_GATEWAY,
            "provider_unreachable",
            "Could not reach the AI provider.",
        )
        .into_response(),
        TitleOutcome::Empty => Failure::error_body(
            StatusCode::BAD_GATEWAY,
            "generation_failed",
            "The AI assistant did not return a usable title.",
        )
        .into_response(),
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
    row: &LlmRow,
    api_key: &str,
    description: &str,
) -> TitleOutcome {
    match run_title(model, &row.base_url, description, api_key).await {
        Some(title) if !title.is_empty() => TitleOutcome::Title(title),
        Some(_) => TitleOutcome::Empty,
        None => TitleOutcome::ProviderFailed,
    }
}

/// Run one title request against the resolved provider and return the
/// cleaned title, or `None` when the provider call fails at any point
/// (the title path collapses every provider failure into the 502 — no
/// classification like the test probe). Credentials travel like the
/// SDKs send them: `x-api-key` for Anthropic, bearer for
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

    fn row() -> LlmRow {
        LlmRow {
            provider_kind: "openai_compatible".to_owned(),
            base_url: "https://8.8.8.8/v1".to_owned(),
            model_name: "m".to_owned(),
            api_key_encrypted: Some(b"blob".to_vec()),
            last_verified_at: None,
        }
    }

    fn object(json: serde_json::Value) -> BodyMap {
        match json {
            Value::Object(map) => map
                .into_iter()
                .map(|(key, value)| (key, BodyField::Json(value)))
                .collect(),
            _ => panic!("not an object"),
        }
    }

    #[test]
    fn serialize_unset_shape_matches_python() {
        let body = serialize_row(None);
        assert_eq!(
            body,
            serde_json::json!({
                "provider_kind": "openai_compatible",
                "base_url": "",
                "model_name": "",
                "has_api_key": false,
                "last_verified_at": null,
            })
        );
    }

    #[test]
    fn put_validation_collects_in_field_order() {
        let existing = row();
        let body = object(serde_json::json!({
            "provider_kind": "x",
            "base_url": "ftp://x/v1",
            "model_name": "",
            "api_key": "short",
        }));
        let errors = validate_put(&body, Some(&existing)).expect_err("invalid");
        let fields: Vec<&str> = errors.iter().map(|(field, _)| field.as_str()).collect();
        assert_eq!(
            fields,
            vec!["provider_kind", "base_url", "model_name", "api_key"]
        );
    }

    #[test]
    fn put_cross_field_requires_base_url_for_openai_compatible() {
        let body = object(serde_json::json!({"model_name": "m"}));
        let errors = validate_put(&body, None).expect_err("missing base_url");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, "base_url");
    }

    #[test]
    fn put_partial_update_keeps_unset_fields_valid() {
        let existing = row();
        let body = object(serde_json::json!({"model_name": "m2"}));
        let validated = validate_put(&body, Some(&existing)).expect("partial valid");
        assert_eq!(validated.model_name.as_deref(), Some("m2"));
        assert_eq!(validated.base_url, None);
    }

    #[test]
    fn probe_classification_matches_run_test_order() {
        assert_eq!(classify_probe(200, "ok"), ProbeOutcome::Ok);
        assert_eq!(
            classify_probe(401, r#"{"error":{"message":"Incorrect API key"}}"#),
            ProbeOutcome::Classified("provider_auth_failed", "API key rejected.")
        );
        assert_eq!(
            classify_probe(404, r#"{"error":{"message":"model not found"}}"#),
            ProbeOutcome::Classified("model_invalid", "Model not accepted by the provider.")
        );
        assert_eq!(
            classify_probe(500, "boom"),
            ProbeOutcome::Classified("provider_unreachable", "Could not reach the endpoint.")
        );
    }

    #[test]
    fn description_coercion_matches_python_str_or() {
        assert_eq!(
            py_description(&serde_json::json!("  hi  ")),
            "  hi  ".to_owned()
        );
        assert_eq!(py_description(&serde_json::json!(null)), "");
        assert_eq!(py_description(&serde_json::json!(0)), "");
        assert_eq!(py_description(&serde_json::json!(false)), "");
        assert_eq!(py_description(&serde_json::json!([])), "");
        assert_eq!(py_description(&serde_json::json!(true)), "True");
        assert_eq!(py_description(&serde_json::json!(5)), "5");
    }
}
