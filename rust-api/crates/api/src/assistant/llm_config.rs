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
//! [`pidash_types::assistant::serializers`], the title path (provider
//! gates, decrypt, provider HTTP) runs through the state-carried
//! [`pidash_services::extensions::AssistantModelSeam`] (CE:
//! [`super::model_seam::CeAssistantSeam`], which reuses
//! [`pidash_services::assistant::llm::resolve_byok_model`] and the
//! [`pidash_services::assistant::title`] request/response pipeline), and
//! the member gate reuses [`super::perm::require_member`]. The
//! config-row service lives in
//! [`pidash_services::assistant::config`] and the probe in
//! [`pidash_services::assistant::probe`] (OVL-07); this module owns the
//! HTTP shell (routes, session auth, PUT validation) and the DRF error
//! rendering (the `pydantic-ai`/`openai`/`anthropic` SDK clients stay in
//! Python; the wire behavior is what is ported).
//!
//! Throttle scopes (`assistant_llm_test` 6/minute,
//! `assistant_llm_generate_title` 20/minute,
//! [`super::throttles::LLM_TEST_THROTTLE`]/[`GENERATE_TITLE_THROTTLE`])
//! are enforced here via [`super::governor::throttle_check`] right after
//! actor resolution (the transcribe precedent): DRF checks throttles
//! before the handler body, hence before the member gate.
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
use pidash_services::assistant::config::{
    self, ConfigDeps, ConfigError, LlmConfigRow, ValidatedPut,
};
use pidash_services::assistant::probe::{self, ProbeOutcome};
use pidash_services::assistant::ssrf::SystemResolver;
use pidash_services::extensions::{ByokTitleInputs, TitleError, TitleRequest};
use pidash_types::assistant::serializers;

use super::common::{
    actor, base_url_blocked, body_get, crypto_config, decrypt_api_key, isoformat, ok_false,
    ok_false_detail, ok_true, parse_body, parse_put_body, pool_of, request_content_type,
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
// The row service lives in [`pidash_services::assistant::config`]
// (OVL-07 moved `fetch_row`/`save_row` there as `get_config` /
// `put_config` / `delete_config`); this module keeps the HTTP shell
// plus the response rendering below.

/// Render `_serialize` (`llm_config.py:23-38`): `Meta.fields` order minus
/// the write-only `api_key`; `last_verified_at` is `isoformat()` or
/// `None`. The unset shape carries the model default provider kind.
fn serialize_row(row: Option<&LlmConfigRow>) -> Value {
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

/// Field-level + cross-field validation in DRF order: every present
/// writable field coerces, checks `max_length`, then runs its custom
/// `validate_<field>`; errors collect in `Meta.fields` order and the
/// cross-field `validate()` runs only when no field failed.
///
/// Public for the cloud overlay's config twin (OVL-07), which copied
/// this function line for line. It moves to
/// [`pidash_services::assistant::config`] once OVL-04's compat crate
/// lands — its field mechanics (`run_char_field`, `validate_choice`)
/// are OVL-04's named unit, and this crate cannot move them without
/// forking that unit's rebase.
pub fn validate_put(
    body: &BodyMap,
    instance: Option<&LlmConfigRow>,
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
    let row = match config::get_config(&pool, actor.id).await {
        Ok(row) => row,
        Err(_) => return Failure::server_error().into_response(),
    };
    crate::license::json_response(serialize_row(row.as_ref()))
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
    let row = match config::get_config(&pool, actor.id).await {
        Ok(row) => row,
        Err(_) => return Failure::server_error().into_response(),
    };
    let validated = match validate_put(&data, row.as_ref()) {
        Ok(validated) => validated,
        Err(errors) => return Failure::field_errors(errors).into_response(),
    };

    let crypto = crypto_config(&state);
    let kms = super::kms::HttpKmsTransport::from_env(&crypto);
    let deps = ConfigDeps {
        pool: &pool,
        crypto: &crypto,
        kms: &kms,
        block_private_urls: state.settings().assistant.block_private_urls,
        ssrf_resolver: &SystemResolver,
    };
    let saved = match config::put_config(&deps, actor.id, row.as_ref(), validated).await {
        Ok(saved) => saved,
        Err(ConfigError::BaseUrlBlocked) => {
            return Failure::error_body(
                StatusCode::BAD_REQUEST,
                "base_url_blocked",
                "That endpoint host is not allowed.",
            )
            .into_response()
        }
        Err(ConfigError::Crypto(err)) => {
            return super::common::crypto_failure(err, |assistant| {
                Failure::assistant_error(assistant).into_response()
            })
        }
        Err(ConfigError::SaveFailed) => {
            return Failure::bare_error(StatusCode::BAD_REQUEST, "invalid").into_response()
        }
    };
    crate::license::json_response(serialize_row(Some(&saved)))
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
    if config::delete_config(&pool, actor.id).await.is_err() {
        return Failure::server_error().into_response();
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

// The probe lives in [`pidash_services::assistant::probe`] (OVL-07
// moved `classify_probe` / `probe_provider` there verbatim).

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
    // DRF `initial()` runs `check_throttles` before the handler body, so
    // a throttled caller answers 429 without touching the provider probe.
    if !super::governor::throttle_check(super::throttles::LLM_TEST_THROTTLE, &actor.id.to_string())
    {
        return super::Denial::throttled().into_response();
    }
    let row = match config::get_config(&pool, actor.id).await {
        Ok(row) => row,
        Err(_) => return Failure::server_error().into_response(),
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
    match probe::probe_provider(&row.provider_kind, &row.base_url, &row.model_name, &api_key).await
    {
        ProbeOutcome::TransportFailure => ok_false("provider_unreachable"),
        ProbeOutcome::Classified(code, detail) => ok_false_detail(code, detail),
        ProbeOutcome::Ok => {
            let now = chrono::Utc::now();
            if let Err(failure) =
                pidash_services::domain::assistant::stamp_llm_config_verified(&pool, actor.id, now)
                    .await
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
    // DRF `initial()` runs `check_throttles` before the handler body, so
    // a throttled caller answers 429 even when the member gate below
    // would also deny it.
    if !super::governor::throttle_check(
        super::throttles::GENERATE_TITLE_THROTTLE,
        &actor.id.to_string(),
    ) {
        return super::Denial::throttled().into_response();
    }
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

    let row = match config::get_config(&pool, actor.id).await {
        Ok(row) => row,
        Err(_) => return Failure::server_error().into_response(),
    };
    // The BYOK row is data for the seam, not a gate: a missing row is a
    // `None` the CE path answers with `llm_config_missing` while the
    // overlay may route elsewhere (same provider as the conversation).
    let byok = row.map(|row| {
        let has_key = has_secret(&row.api_key_encrypted);
        let blocked = base_url_blocked(&state, &row.base_url);
        ByokTitleInputs {
            model_name: row.model_name,
            provider_kind: row.provider_kind,
            base_url: row.base_url,
            base_url_blocked: blocked,
            api_key_encrypted: row.api_key_encrypted,
            has_api_key: has_key,
        }
    });
    let request = TitleRequest {
        user_id: actor.id,
        description,
        byok,
    };
    match state.assistant_seam().generate_title(request).await {
        Ok(title) if !title.is_empty() => {
            crate::license::json_response(serde_json::json!({"title": title}))
        }
        // `if not title` (`llm_config.py:152`): an empty model reply is
        // the `generation_failed` 502, whatever the provider.
        Ok(_) => Failure::error_body(
            StatusCode::BAD_GATEWAY,
            "generation_failed",
            "The AI assistant did not return a usable title.",
        )
        .into_response(),
        Err(TitleError::Assistant(err)) => Failure::assistant_error(&err).into_response(),
        Err(TitleError::ServerError) => Failure::server_error().into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> LlmConfigRow {
        LlmConfigRow {
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

/// Live scratch-DB proof that the title handler consults the state-carried
/// seam (PIDASHCONV-914), env-gated like the `handlers_members` live
/// tests: without `DATABASE_URL` these skip. A forged session drives the
/// handler directly; the spy seam records the request it receives, so the
/// tests pin both directions — the canned title on the wire, and the
/// user/description/BYOK inputs the handler passed in. No BYOK row is
/// seeded: `None` is exactly the case the overlay routes elsewhere while
/// CE answers `llm_config_missing`.
#[cfg(test)]
mod live_tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use pidash_services::extensions::{AssistantModelSeam, SeamFuture};

    /// Spy seam: records the title request, answers a canned title.
    #[derive(Debug, Default)]
    struct TitleSpy {
        seen: Mutex<Option<TitleRequest>>,
    }

    impl AssistantModelSeam for TitleSpy {
        fn generate_title(
            &self,
            request: TitleRequest,
        ) -> SeamFuture<'_, Result<String, TitleError>> {
            *self.seen.lock().expect("spy lock") = Some(request);
            Box::pin(async move { Ok("Canned title".to_owned()) })
        }
    }

    async fn live_state(seam: Option<Arc<dyn AssistantModelSeam>>) -> Option<(AppState, String)> {
        let url = std::env::var("DATABASE_URL").ok()?;
        let cfg = pidash_db::config::DbConfig::new(url, 5).expect("db config");
        let pools = pidash_db::Pools::connect(&cfg, None)
            .await
            .expect("connect pools");
        let settings = pidash_db::config::Settings::test_defaults();
        let mut state = AppState::with_settings("test", settings).with_pools(pools);
        if let Some(seam) = seam {
            state = state.with_assistant_seam(seam);
        }
        let secret = state.settings().secret_key.clone();
        Some((state, secret))
    }

    /// Seed one active user; returns its id. Column list mirrors the
    /// contract harness `create_user` (the `handlers_members` precedent).
    async fn seed_user(pool: &sqlx::PgPool, first: &str) -> uuid::Uuid {
        let id = uuid::Uuid::new_v4();
        let email = format!("seam-{}-{first}@example.com", id.simple());
        sqlx::query(
            "INSERT INTO users (id, password, username, email, first_name, last_name,
                avatar, date_joined, created_at, updated_at, last_location,
                created_location, is_superuser, is_managed, is_password_expired,
                is_active, is_staff, is_email_verified, is_password_autoset, token,
                user_timezone, last_login_ip, last_logout_ip, last_login_medium,
                last_login_uagent, is_bot, display_name, is_email_valid,
                is_password_reset_required)
             VALUES ($1, '!', $2, $2, $3, 'User',
                '', now(), now(), now(), '',
                '', false, false, false,
                true, false, true, false, '',
                'UTC', '', '', '',
                '', false, $4, true,
                false)",
        )
        .bind(id)
        .bind(&email)
        .bind(first)
        .bind(format!("{first} User"))
        .execute(pool)
        .await
        .expect("seed user");
        id
    }

    /// Seed one workspace with `user_id` as an admin member; returns the
    /// slug. Column lists mirror the contract harness.
    async fn seed_membership(pool: &sqlx::PgPool, user_id: uuid::Uuid) -> String {
        let tag = uuid::Uuid::new_v4().simple().to_string();
        let short = tag[..8].to_owned();
        let ws_id = uuid::Uuid::new_v4();
        let slug = format!("seam-{short}");
        sqlx::query(
            "INSERT INTO workspaces (id, name, slug, owner_id, created_by_id,
                updated_by_id, timezone, background_color, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $4, $4, 'UTC', '', now(), now())",
        )
        .bind(ws_id)
        .bind(format!("Seam {short}"))
        .bind(&slug)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed workspace");
        sqlx::query(
            "INSERT INTO workspace_members (id, workspace_id, member_id, role,
                is_active, view_props, default_props, issue_props,
                explored_features, getting_started_checklist, tips,
                created_at, updated_at)
             VALUES ($1, $2, $3, 20,
                true, '{}', '{}', '{}',
                '{}', '{}', '{}',
                now(), now())",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(ws_id)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed workspace member");
        slug
    }

    fn hex_encode(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
        out
    }

    /// `user.get_session_auth_hash()` for the seeded `'!'` password field
    /// (the `resolve_actor` verification, recomputed, not copied).
    fn session_hash(secret_key: &str) -> String {
        use hmac::{Hmac, Mac};
        use sha2::{Digest, Sha256};
        let key = Sha256::digest(
            [
                pidash_compat::http::SESSION_AUTH_HASH_SALT.as_bytes(),
                secret_key.as_bytes(),
            ]
            .concat(),
        );
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&key).expect("HMAC-SHA256 accepts any key length");
        mac.update(b"!");
        hex_encode(&mac.finalize().into_bytes())
    }

    fn forged_session(
        user_id: uuid::Uuid,
        secret_key: &str,
    ) -> axum::Extension<crate::middleware::SessionHandle> {
        let mut session = crate::middleware::RequestSession::empty();
        session.set(
            "_auth_user_id".to_owned(),
            Value::String(user_id.to_string()),
        );
        session.set(
            "_auth_user_backend".to_owned(),
            Value::String(crate::license::MODEL_BACKEND.to_owned()),
        );
        session.set(
            "_auth_user_hash".to_owned(),
            Value::String(session_hash(secret_key)),
        );
        axum::Extension(crate::middleware::SessionHandle::new(session))
    }

    async fn body_json(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body");
        let body: Value = serde_json::from_slice(&bytes).expect("json body");
        (status, body)
    }

    fn json_headers() -> axum::http::HeaderMap {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse().expect("content type"),
        );
        headers
    }

    #[tokio::test]
    async fn title_serves_the_installed_seam() {
        let spy = Arc::new(TitleSpy::default());
        let Some((state, secret)) = live_state(Some(spy.clone())).await else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = state.pools().expect("pools").primary().clone();
        let user_id = seed_user(&pool, "SeamTitle").await;
        let slug = seed_membership(&pool, user_id).await;
        let extension = forged_session(user_id, &secret);
        let body_in = axum::body::Bytes::from_static(br#"{"description":"Fix the thing"}"#);
        let (status, body) = body_json(
            post_generate_title(
                State(state),
                Path(slug),
                Some(extension),
                json_headers(),
                body_in,
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["title"], "Canned title");
        // And the handler passed the seam exactly this call's inputs.
        let seen = spy.seen.lock().expect("spy lock").clone().expect("called");
        assert_eq!(seen.user_id, user_id);
        assert_eq!(seen.description, "Fix the thing");
        assert!(seen.byok.is_none(), "no BYOK row was seeded");
    }

    #[tokio::test]
    async fn title_without_a_row_answers_like_ce() {
        // No `with_assistant_seam`: the default CE seam answers a missing
        // row with the same `llm_config_missing` the handler rendered
        // before the seam existed.
        let Some((state, secret)) = live_state(None).await else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = state.pools().expect("pools").primary().clone();
        let user_id = seed_user(&pool, "SeamTitleCe").await;
        let slug = seed_membership(&pool, user_id).await;
        let extension = forged_session(user_id, &secret);
        let body_in = axum::body::Bytes::from_static(br#"{"description":"Fix the thing"}"#);
        let (status, body) = body_json(
            post_generate_title(
                State(state),
                Path(slug),
                Some(extension),
                json_headers(),
                body_in,
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"], "llm_config_missing");
        assert_eq!(
            body["detail"],
            "No LLM provider is configured for this user."
        );
    }
}
