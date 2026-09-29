#![forbid(unsafe_code)]

//! BYO speech-to-text config handlers (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/views/stt_config.py:1-147`:
//!
//! * `UserSTTConfigEndpoint` (`:44-81`) — GET/PUT/DELETE on
//!   `users/me/ai-assistant/stt-config/` with `_serialize` (`:28-41`).
//! * `UserSTTConfigTestEndpoint` (`:88-111`) — POST on
//!   `users/me/ai-assistant/stt-config/test/` with `_run_test`
//!   (`:114-147`).
//!
//! The surface mirrors the LLM config (`super::llm_config`) minus the
//! provider selector: dictation has a single OpenAI-compatible provider,
//! so there is no `provider_kind` field and `base_url` is always required
//! (`serializers.py:78-120`).
//!
//! Fixture ids: F-A6-01 (config shapes), F-A6-04 (key encrypt on save),
//! F-A6-05 (base_url SSRF check on save), F-A6-07 (test throttle).
//!
//! Throttle scope (`assistant_stt_test` 6/minute,
//! [`super::throttles::STT_TEST_THROTTLE`]) is recorded but not enforced,
//! same precedent as the LLM handlers (see `super::llm_config`).
//!
//! Ported bugs (translate, don't redesign):
//!
//! * `cfg.save()` failures — even operational ones — answer 400
//!   `{"error": "invalid"}` (`stt_config.py:74-76`).

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::Value;

use pidash_db::assistant::models::{has_secret, user_stt_config};
use pidash_types::assistant::serializers;

use super::common::{
    actor, base_url_blocked, body_get, crypto_config, decrypt_api_key, encrypt_api_key, isoformat,
    ok_false, ok_false_detail, ok_true, parse_put_body, pool_of, request_content_type,
    run_char_field, BodyField, BodyMap, CharFieldRules, Failure,
};
use crate::state::AppState;

/// `assistant/urls.py:70-73` (under the `api/` include).
pub const CONFIG_PATH: &str = "/api/users/me/ai-assistant/stt-config/";
/// `assistant/urls.py:74-78`.
pub const CONFIG_TEST_PATH: &str = "/api/users/me/ai-assistant/stt-config/test/";

/// Owned STT-config routes. The `transcribe/` path belongs to a sibling
/// issue and stays unmatched here (it proxies); every unowned method on
/// the owned paths also proxies so Django answers its own 405-after-auth
/// and metadata OPTIONS byte for byte.
pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route(
            CONFIG_PATH,
            owned_config(get(get_config).put(put_config).delete(delete_config)),
        )
        .route(CONFIG_TEST_PATH, owned_test(post(post_test)))
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

// ---------------------------------------------------------------------------
// config rows (`UserSTTConfig.objects.filter(user=...)`, `models.py:265-293`)
// ---------------------------------------------------------------------------

/// One `assistant_user_stt_config` row as the handlers read it.
#[derive(Debug, Clone)]
struct SttRow {
    base_url: String,
    model_name: String,
    api_key_encrypted: Option<Vec<u8>>,
    last_verified_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Decoded config-row tuple: base URL, model, encrypted key,
/// verification timestamp.
type SttRowTuple = (
    String,
    String,
    Option<Vec<u8>>,
    Option<chrono::DateTime<chrono::Utc>>,
);

async fn fetch_row(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<Option<SttRow>, Failure> {
    let row: Option<SttRowTuple> = sqlx::query_as(
        r#"SELECT base_url, model_name, api_key_encrypted, last_verified_at
           FROM assistant_user_stt_config WHERE user_id = $1 LIMIT 1"#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Failure::server_error())?;
    Ok(row.map(
        |(base_url, model_name, api_key_encrypted, last_verified_at)| SttRow {
            base_url,
            model_name,
            api_key_encrypted,
            last_verified_at,
        },
    ))
}

/// `cfg.save()` (`stt_config.py:73`): a full-row write that refreshes
/// `updated_at`; any failure answers 400 `{"error": "invalid"}`
/// (`:74-76`).
async fn save_row(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
    base_url: &str,
    model_name: &str,
    api_key_encrypted: &Option<Vec<u8>>,
    existed: bool,
) -> Result<(), Failure> {
    let now = chrono::Utc::now();
    let result = if existed {
        sqlx::query(
            r#"UPDATE assistant_user_stt_config
               SET base_url = $1, model_name = $2, api_key_encrypted = $3, updated_at = $4
               WHERE user_id = $5"#,
        )
        .bind(base_url)
        .bind(model_name)
        .bind(api_key_encrypted)
        .bind(now)
        .bind(user_id)
        .execute(pool)
        .await
    } else {
        sqlx::query(
            r#"INSERT INTO assistant_user_stt_config
               (user_id, base_url, model_name, api_key_encrypted,
                last_verified_at, created_at, updated_at)
               VALUES ($1, $2, $3, $4, NULL, $5, $6)"#,
        )
        .bind(user_id)
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

/// Render `_serialize` (`stt_config.py:28-41`): no `provider_kind` field;
/// the write-only `api_key` never appears.
fn serialize_row(row: Option<&SttRow>) -> Value {
    match row {
        None => serde_json::json!({
            "base_url": user_stt_config::DEFAULT_BASE_URL,
            "model_name": user_stt_config::DEFAULT_MODEL_NAME,
            "has_api_key": false,
            "last_verified_at": null,
        }),
        Some(row) => serde_json::json!({
            "base_url": row.base_url,
            "model_name": row.model_name,
            "has_api_key": has_secret(&row.api_key_encrypted),
            "last_verified_at": row.last_verified_at.as_ref().map(isoformat),
        }),
    }
}

// ---------------------------------------------------------------------------
// PUT validation (`UserSTTConfigSerializer`, `serializers.py:78-120`)
// ---------------------------------------------------------------------------

/// Validated PUT fields; `None` is "omitted".
#[derive(Debug, Default)]
struct ValidatedPut {
    base_url: Option<String>,
    model_name: Option<String>,
    api_key: Option<String>,
}

/// Field-level + cross-field validation in DRF order (same shape as the
/// LLM surface; the cross-field rule only requires `base_url`,
/// `serializers.py:116-120`).
fn validate_put(
    body: &BodyMap,
    instance: Option<&SttRow>,
) -> Result<ValidatedPut, Vec<(String, Vec<String>)>> {
    let mut errors: Vec<(String, Vec<String>)> = Vec::new();
    let mut out = ValidatedPut::default();

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

    if let Err((field, message)) = serializers::validate_stt_config(
        out.base_url.as_deref(),
        instance.map(|row| row.base_url.as_str()),
    ) {
        errors.push((field.to_owned(), vec![message.to_owned()]));
        return Err(errors);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// handlers
// ---------------------------------------------------------------------------

/// `GET users/me/ai-assistant/stt-config/` (`stt_config.py:45-47`).
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

/// `PUT users/me/ai-assistant/stt-config/` (`stt_config.py:49-77`).
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
    let model_name = validated.model_name.or(stored_model).unwrap_or_default();
    if let Err(failure) = save_row(
        &pool,
        &actor.id,
        &base_url_stored,
        &model_name,
        &api_key_encrypted,
        existed,
    )
    .await
    {
        return failure.into_response();
    }
    let saved = SttRow {
        base_url: base_url_stored,
        model_name,
        api_key_encrypted,
        last_verified_at: stored_verified,
    };
    crate::license::json_response(&serialize_row(Some(&saved)))
}

/// `DELETE users/me/ai-assistant/stt-config/` (`stt_config.py:79-81`).
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
    if let Err(failure) = sqlx::query(r#"DELETE FROM assistant_user_stt_config WHERE user_id = $1"#)
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
// connection test (`UserSTTConfigTestEndpoint`, `stt_config.py:88-147`)
// ---------------------------------------------------------------------------

/// Outcome of the transcription probe: success or a classified failure
/// with its `(code, detail)`. Transport errors (`httpx.HTTPError`,
/// `stt_config.py:135-136`) classify as `provider_unreachable` *with*
/// detail here — the bare outer-`except` shape (`:104-105`) only covers
/// non-HTTP failures, which have no reqwest equivalent on this path.
#[derive(Debug, PartialEq)]
enum ProbeOutcome {
    Ok,
    Classified(&'static str, &'static str),
}

/// Multipart boundary for the probe body. The boundary value never
/// reaches a contract assertion (provider round-trips need a live STT
/// endpoint), so it is static rather than random.
const PROBE_BOUNDARY: &str = "pidash-stt-probe-0000000000000000";

/// Minimal, intentionally invalid transcription body
/// (`stt_config.py:126-131`): a one-byte `probe.wav` plus the configured
/// model name. Field order matches `requests` (`data` before `files`):
/// a well-behaved server answers 4xx once it has accepted the
/// credential, which is all the test needs.
fn probe_body(model_name: &str) -> (String, Vec<u8>) {
    let content_type = format!("multipart/form-data; boundary={PROBE_BOUNDARY}");
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{PROBE_BOUNDARY}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\n{model_name}\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(
        format!(
            "--{PROBE_BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"probe.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.push(0x00);
    body.extend_from_slice(format!("\r\n--{PROBE_BOUNDARY}--\r\n").as_bytes());
    (content_type, body)
}

/// Classify a probe round-trip (`_run_test`, `stt_config.py:133-147`):
/// transport errors are unreachable; 401/403 is auth; 404 is a missing
/// endpoint/model; 5xx is unreachable; 2xx or any other 4xx proves the
/// endpoint answered and the key was accepted. Only the response class
/// is inspected — bodies are never surfaced.
fn classify_probe(status: u16) -> ProbeOutcome {
    if status == 401 || status == 403 {
        ProbeOutcome::Classified("provider_auth_failed", "API key rejected.")
    } else if status == 404 {
        ProbeOutcome::Classified(
            "model_invalid",
            "Transcription endpoint or model not found.",
        )
    } else if status >= 500 {
        ProbeOutcome::Classified(
            "provider_unreachable",
            "The endpoint returned a server error.",
        )
    } else {
        ProbeOutcome::Ok
    }
}

async fn probe_provider(base_url: &str, model_name: &str, api_key: &str) -> ProbeOutcome {
    let (content_type, body) = probe_body(model_name);
    let url = format!("{}/audio/transcriptions", base_url.trim_end_matches('/'));
    // No redirect following: the ported call is a bare `httpx.post`
    // (redirects off by default), so a 302 proves reachability as-is
    // instead of being chased to whatever it points at.
    let response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("probe client builds")
        .post(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", content_type)
        .body(body)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;
    match response {
        Err(_) => ProbeOutcome::Classified("provider_unreachable", "Could not reach the endpoint."),
        Ok(response) => classify_probe(response.status().as_u16()),
    }
}

/// `POST users/me/ai-assistant/stt-config/test/`
/// (`stt_config.py:91-111`).
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
        return ok_false("stt_config_missing");
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
    match probe_provider(&row.base_url, &row.model_name, &api_key).await {
        ProbeOutcome::Classified(code, detail) => ok_false_detail(code, detail),
        ProbeOutcome::Ok => {
            let now = chrono::Utc::now();
            if let Err(failure) = sqlx::query(
                r#"UPDATE assistant_user_stt_config SET last_verified_at = $1 WHERE user_id = $2"#,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> SttRow {
        SttRow {
            base_url: "https://8.8.8.8/v1".to_owned(),
            model_name: "whisper-1".to_owned(),
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
    fn serialize_shape_has_no_provider_kind() {
        let unset = serialize_row(None);
        assert!(unset.get("provider_kind").is_none());
        assert_eq!(unset.get("has_api_key"), Some(&Value::Bool(false)));
        let set = serialize_row(Some(&row()));
        assert_eq!(
            set,
            serde_json::json!({
                "base_url": "https://8.8.8.8/v1",
                "model_name": "whisper-1",
                "has_api_key": true,
                "last_verified_at": null,
            })
        );
    }

    #[test]
    fn put_validation_mirrors_llm_minus_provider() {
        let existing = row();
        let body = object(serde_json::json!({
            "base_url": "ftp://x/v1",
            "model_name": "",
            "api_key": "short",
        }));
        let errors = validate_put(&body, Some(&existing)).expect_err("invalid");
        let fields: Vec<&str> = errors.iter().map(|(field, _)| field.as_str()).collect();
        assert_eq!(fields, vec!["base_url", "model_name", "api_key"]);
    }

    #[test]
    fn put_cross_field_always_requires_base_url() {
        let body = object(serde_json::json!({"model_name": "w"}));
        let errors = validate_put(&body, None).expect_err("missing base_url");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, "base_url");
        // ...but a stored URL carries a key-only update.
        let existing = row();
        let body = object(serde_json::json!({"model_name": "whisper-large"}));
        assert!(validate_put(&body, Some(&existing)).is_ok());
    }

    #[test]
    fn probe_classification_matches_run_test() {
        assert_eq!(classify_probe(200), ProbeOutcome::Ok);
        assert_eq!(classify_probe(400), ProbeOutcome::Ok);
        assert_eq!(
            classify_probe(401),
            ProbeOutcome::Classified("provider_auth_failed", "API key rejected.")
        );
        assert_eq!(
            classify_probe(403),
            ProbeOutcome::Classified("provider_auth_failed", "API key rejected.")
        );
        assert_eq!(
            classify_probe(404),
            ProbeOutcome::Classified(
                "model_invalid",
                "Transcription endpoint or model not found."
            )
        );
        assert_eq!(
            classify_probe(500),
            ProbeOutcome::Classified(
                "provider_unreachable",
                "The endpoint returned a server error."
            )
        );
    }

    #[test]
    fn probe_body_carries_probe_wav_and_model() {
        let (content_type, body) = probe_body("whisper-1");
        assert!(content_type.starts_with("multipart/form-data; boundary="));
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("name=\"model\"\r\n\r\nwhisper-1\r\n"));
        assert!(text.contains("name=\"file\"; filename=\"probe.wav\""));
        assert!(text.contains("Content-Type: audio/wav"));
    }
}
