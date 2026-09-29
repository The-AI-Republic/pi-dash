//! Voice-dictation transcribe handler (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/views/transcribe.py:1-194`
//! (`AssistantTranscribeEndpoint`, route
//! `users/me/ai-assistant/transcribe/` from `assistant/urls.py:83-87`).
//!
//! Flow, in order (`post`, `:66-130`): the STT config gate (422
//! `stt_config_missing`) runs before the upload is touched; the cheap
//! `Content-Length` pre-filter answers `audio_too_large` (413) before the
//! multipart is parsed; a missing `file` part answers `no_audio` (400);
//! an over-25 MB payload answers `audio_too_large`; otherwise the user's
//! BYO provider is resolved through the CE seam and the audio forwarded
//! as multipart to `{base_url}/audio/transcriptions`, answering `{text}`.
//!
//! Layering: STT presence/resolution via
//! `pidash_services::assistant::seams`, decryption via
//! `pidash_services::assistant::crypto`, the SSRF re-check via
//! `pidash_services::assistant::ssrf`, framing via
//! [`super::multipart`], auth/throttle/deny plumbing via the
//! [`super`] shell. This module owns the HTTP shell (route, session
//! auth, throttle, multipart handling, provider POST), the SQL text, and
//! the response rendering. The audio is forwarded for the one request
//! and dropped — it never touches the DB.
//!
//! Registration is the cutover granularity (same rule as the `space`,
//! `loop` and `prompting` families): POST serves from Rust while every
//! other method on the path proxies to Django, preserving DRF's
//! authenticate-before-method order byte for byte. `HEAD` proxies too —
//! Django has no GET here, so its own 405 answers.

use std::sync::OnceLock;

use axum::body::Body;
use axum::extract::State;
use axum::http::header;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Router;
use http_body_util::BodyExt as _;
use serde_json::Value;

use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::multipart;
use super::{error_response, json_body, pool_of, request_actor, Denial};

/// `users/me/ai-assistant/transcribe/` (under the `api/` include).
pub const TRANSCRIBE_PATH: &str = "/api/users/me/ai-assistant/transcribe/";

/// Register the owned POST path. Every other method proxies to Django
/// (its 405-after-auth and metadata OPTIONS live there).
pub fn routes() -> Router<AppState> {
    Router::new().route(
        TRANSCRIBE_PATH,
        axum::routing::post(post_transcribe)
            .get(crate::edge::proxy)
            .head(crate::edge::proxy)
            .put(crate::edge::proxy)
            .patch(crate::edge::proxy)
            .delete(crate::edge::proxy)
            .options(crate::edge::proxy),
    )
}

/// OpenHub's gateway caps transcription uploads at 25 MB
/// (`transcribe.py:41`); anything larger is guaranteed to fail on the
/// cloud path. Kept at (not below) that ceiling so a self-hoster with a
/// more generous provider is not needlessly restricted.
pub const MAX_AUDIO_BYTES: u64 = 25 * 1024 * 1024;

/// Multipart framing + the small optional text fields add a little on top
/// of the raw audio, so the cheap Content-Length pre-filter allows this
/// much slack; the authoritative check is against the parsed file's own
/// size (`transcribe.py:46`).
pub const CONTENT_LENGTH_SLACK: u64 = 1024 * 1024;

/// Response formats that return plain text rather than a JSON object
/// with a `text` field (`transcribe.py:50`).
pub const TEXT_FORMATS: &[&str] = &["text", "srt", "vtt"];

/// Connect timeout for the provider POST (`transcribe.py:54`).
pub const CONNECT_TIMEOUT_SECS: u64 = 10;
/// Read/write timeout for the provider POST (`transcribe.py:54`).
/// `reqwest` has no write-timeout knob, so the 120 s applies to reads;
/// the upload itself is bounded by the 25 MB cap.
pub const READ_TIMEOUT_SECS: u64 = 120;

/// Cheap pre-filter on Content-Length (`_content_length_over_cap`,
/// `transcribe.py:133-140`): reject an oversize body before parsing the
/// multipart at all. Absent or unparseable means "no opinion" (false),
/// exactly like the Python (`if not raw: False`, `except → False`).
pub fn content_length_over_cap(content_length: Option<&str>) -> bool {
    let raw = match content_length {
        Some(raw) if !raw.is_empty() => raw,
        _ => return false,
    };
    match raw.trim().parse::<u64>() {
        Ok(len) => len > MAX_AUDIO_BYTES + CONTENT_LENGTH_SLACK,
        Err(_) => false,
    }
}

/// Map a provider HTTP status onto the stable dictation error taxonomy
/// (`_classify`, `transcribe.py:150-177`): `(error_code, http_status,
/// detail)` for a failure, `None` for success. Deliberately small and
/// dictation-specific so the composer never renders a dictation failure
/// as a chat failure.
pub fn classify_provider_status(code: u16) -> Option<(&'static str, StatusCode, &'static str)> {
    if code == 401 || code == 403 {
        return Some((
            "provider_auth_failed",
            StatusCode::BAD_GATEWAY,
            "The dictation provider rejected the API key.",
        ));
    }
    if code == 404 {
        return Some((
            "model_invalid",
            StatusCode::BAD_REQUEST,
            "The transcription model or endpoint was not found.",
        ));
    }
    if code == 413 {
        return Some((
            "audio_too_large",
            StatusCode::PAYLOAD_TOO_LARGE,
            "The provider rejected the recording as too large.",
        ));
    }
    if code >= 500 || code == 429 {
        return Some((
            "provider_unreachable",
            StatusCode::BAD_GATEWAY,
            "The dictation provider is unavailable.",
        ));
    }
    if code >= 400 {
        return Some((
            "transcription_failed",
            StatusCode::BAD_GATEWAY,
            "The dictation provider could not transcribe the audio.",
        ));
    }
    None
}

/// Pull the transcript out of the provider response, whatever its shape
/// (`_extract_text`, `transcribe.py:180-194`): `json`/`verbose_json`
/// return an object with a `text` field; `text` (and subtitle formats)
/// return the transcript as the raw body.
///
/// Returns a [`Value`] (not a [`String`]) because the Python keeps a
/// truthy non-string `text` as-is (`payload.get("text", "") or ""`):
/// only falsy values (`null`, `""`, `0`, `false`, `[]`, `{}`) collapse
/// to `""`; anything else is preserved verbatim.
pub fn extract_text_value(body: &[u8], response_format: &str) -> Value {
    if TEXT_FORMATS.contains(&response_format) {
        return Value::String(String::from_utf8_lossy(body).into_owned());
    }
    let payload: Value = match serde_json::from_slice(body) {
        Ok(payload) => payload,
        Err(_) => return Value::String(String::from_utf8_lossy(body).into_owned()),
    };
    match payload.get("text") {
        None | Some(Value::Null) => Value::String(String::new()),
        Some(Value::String(text)) => Value::String(text.clone()),
        Some(value) if is_falsy(value) => Value::String(String::new()),
        Some(value) => value.clone(),
    }
}

/// Python truthiness for a JSON scalar/container (no NaN/Infinity in
/// strict JSON, so numbers reduce to zero vs non-zero).
fn is_falsy(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(hit) => !hit,
        Value::Number(num) => {
            num.as_i64() == Some(0) || num.as_u64() == Some(0) || num.as_f64() == Some(0.0)
        }
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields.is_empty(),
    }
}

/// `(request.data.get(key) or "").strip()` (`transcribe.py:98-103`):
/// the last text part wins (Django's `QueryDict.get`); missing or blank
/// means "not passed".
pub fn optional_text_field(parts: &[multipart::Part], name: &str) -> Option<String> {
    let raw = parts
        .iter()
        .rev()
        .find(|part| part.name == name && !multipart::is_file(part))
        .map(|part| String::from_utf8_lossy(&part.body).into_owned())?;
    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// `provider.base_url.rstrip("/") + "/audio/transcriptions"`
/// (`transcribe.py:105`).
pub fn provider_transcriptions_url(base_url: &str) -> String {
    format!("{}/audio/transcriptions", base_url.trim_end_matches('/'))
}

/// Shared outbound client (connect 10 s, read 120 s). `reqwest` has no
/// write-timeout knob; the 25 MB cap bounds the upload instead.
fn provider_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .read_timeout(std::time::Duration::from_secs(READ_TIMEOUT_SECS))
            .build()
            .expect("transcribe provider client builds")
    })
}

/// `POST` an audio file, get back `{text}` (`post`, `:66-130`).
async fn post_transcribe(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    extension: Option<axum::Extension<SessionHandle>>,
    body: Body,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match request_actor(&state, &pool, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    // DRF `initial()` runs `check_throttles` before the handler body, so
    // a throttled caller answers 429 even when the config gate below
    // would also deny it.
    if !super::governor::throttle_check(
        super::throttles::TRANSCRIBE_THROTTLE,
        &actor.id.to_string(),
    ) {
        return Denial::throttled().into_response();
    }

    // Gate before touching the upload: no usable config → a code the
    // composer turns into a "configure dictation in Settings" state.
    let stt = match fetch_stt_row(&pool, &actor.id).await {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    if !pidash_services::assistant::seams::has_usable_stt_config(stt.has_api_key) {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            &serde_json::json!({
                "error": "stt_config_missing",
                "detail": "Configure dictation in Settings.",
            }),
        );
    }

    // Cheap pre-filter on Content-Length, then the authoritative parse.
    let content_length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok());
    if content_length_over_cap(content_length) {
        return audio_too_large();
    }
    let raw = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return Denial::ServerError.into_response(),
    };
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let parts = multipart::parse(content_type, &raw);
    let upload = parts
        .iter()
        .rev()
        .find(|part| part.name == "file" && multipart::is_file(part));
    let Some(upload) = upload else {
        return error_response(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({
                "error": "no_audio",
                "detail": "No audio file was uploaded.",
            }),
        );
    };
    if upload.body.len() as u64 > MAX_AUDIO_BYTES {
        return audio_too_large();
    }

    let provider = match resolve_provider(&state, &stt).await {
        Ok(provider) => provider,
        Err(response) => return response,
    };

    // OpenAI-compatible multipart body: model is server-resolved;
    // language and response_format are optional passthroughs.
    let language = optional_text_field(&parts, "language");
    let response_format = optional_text_field(&parts, "response_format");
    let filename = upload
        .filename
        .as_deref()
        .filter(|name| !name.is_empty())
        .unwrap_or("audio");
    let file_content_type = upload
        .content_type
        .as_deref()
        .filter(|kind| !kind.is_empty())
        .unwrap_or("application/octet-stream");
    let (out_content_type, out_body) = multipart::encode(
        &provider.model,
        language.as_deref(),
        response_format.as_deref(),
        filename,
        file_content_type,
        &upload.body,
    );
    let url = provider_transcriptions_url(&provider.base_url);
    let upstream = match provider_client()
        .post(url)
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", provider.api_key),
        )
        .header(header::CONTENT_TYPE, out_content_type)
        .body(out_body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => {
            // Never echo the raw error — it may reveal an internal host.
            return error_response(
                StatusCode::BAD_GATEWAY,
                &serde_json::json!({
                    "error": "provider_unreachable",
                    "detail": "Could not reach the dictation provider.",
                }),
            );
        }
    };
    let status = upstream.status().as_u16();
    let payload = match upstream.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                &serde_json::json!({
                    "error": "provider_unreachable",
                    "detail": "Could not reach the dictation provider.",
                }),
            );
        }
    };
    if let Some((code, http_status, detail)) = classify_provider_status(status) {
        return error_response(
            http_status,
            &serde_json::json!({"error": code, "detail": detail}),
        );
    }
    json_body(
        StatusCode::OK,
        &serde_json::json!({
            "text": extract_text_value(&payload, response_format.as_deref().unwrap_or("")),
        }),
    )
}

/// `{"error":"audio_too_large", ...}` 413 (`_audio_too_large`, `:143`).
fn audio_too_large() -> Response {
    error_response(
        StatusCode::PAYLOAD_TOO_LARGE,
        &serde_json::json!({
            "error": "audio_too_large",
            "detail": "Recording exceeds the 25 MB limit.",
        }),
    )
}

/// One STT config row (`UserSTTConfig.objects.filter(user=…).first()`),
/// reduced to what the seam needs. `has_api_key` is Python truthiness
/// over the encrypted bytes (empty reads as missing).
struct SttRow {
    base_url: String,
    model_name: String,
    api_key_encrypted: Option<Vec<u8>>,
    has_api_key: bool,
}

async fn fetch_stt_row(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<SttRow, Denial> {
    let row = sqlx::query(
        "SELECT \"base_url\", \"model_name\", \"api_key_encrypted\" \
         FROM \"assistant_user_stt_config\" WHERE \"user_id\" = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    use sqlx::Row as _;
    Ok(match row {
        Some(row) => {
            let api_key_encrypted: Option<Vec<u8>> =
                row.try_get("api_key_encrypted").unwrap_or(None);
            let has_api_key = pidash_db::assistant::models::has_secret(&api_key_encrypted);
            SttRow {
                base_url: row.try_get("base_url").unwrap_or_default(),
                model_name: row.try_get("model_name").unwrap_or_default(),
                api_key_encrypted,
                has_api_key,
            }
        }
        None => SttRow {
            base_url: String::new(),
            model_name: String::new(),
            api_key_encrypted: None,
            has_api_key: false,
        },
    })
}

/// `resolve_stt_provider(request.user)` (`:90-93`): the SSRF guard is
/// re-run here, at execution time, rather than trusting the save-time
/// check. `AssistantError` renders as its code/detail/status; an
/// operational crypto failure (never an `AssistantError` in Python)
/// answers the generic 500.
#[allow(clippy::result_large_err)]
async fn resolve_provider(
    state: &AppState,
    stt: &SttRow,
) -> Result<pidash_services::assistant::seams::ResolvedSttProvider, Response> {
    let blocked = !stt.base_url.is_empty()
        && pidash_services::assistant::ssrf::is_blocked(
            &stt.base_url,
            state.settings().assistant.block_private_urls,
            &pidash_services::assistant::ssrf::SystemResolver,
        );
    let encrypted = stt.api_key_encrypted.clone().unwrap_or_default();
    let outcome = pidash_services::assistant::seams::resolve_stt_provider(
        stt.has_api_key,
        &stt.base_url,
        &stt.model_name,
        blocked,
        || {
            super::decrypt_secret(&encrypted).map_err(|err| match err {
                super::SecretError::Assistant(error) => error,
                // An operational crypto failure is never an
                // `AssistantError` in Python: it propagates out of the view
                // to the generic 500. `Internal` is only the carrier here —
                // the match below renders it as the 500 envelope.
                super::SecretError::Transport(_) => {
                    pidash_types::assistant::errors::AssistantError::Internal(
                        "dictation credential unreadable".to_string(),
                    )
                }
            })
        },
    );
    match outcome {
        Ok(provider) => Ok(provider),
        Err(pidash_types::assistant::errors::AssistantError::Internal(_)) => {
            Err(Denial::ServerError.into_response())
        }
        Err(error) => Err(super::assistant_error_response(&error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_length_prefilter_matches_python() {
        assert!(!content_length_over_cap(None));
        assert!(!content_length_over_cap(Some("")));
        assert!(!content_length_over_cap(Some("bogus")));
        assert!(!content_length_over_cap(Some(
            &(MAX_AUDIO_BYTES + CONTENT_LENGTH_SLACK).to_string()
        )));
        assert!(content_length_over_cap(Some(
            &(MAX_AUDIO_BYTES + CONTENT_LENGTH_SLACK + 1).to_string()
        )));
    }

    #[test]
    fn classify_matches_the_taxonomy_table() {
        assert_eq!(classify_provider_status(200), None);
        assert_eq!(classify_provider_status(201), None);
        let (code, status, _) = classify_provider_status(401).expect("mapped");
        assert_eq!(
            (code, status),
            ("provider_auth_failed", StatusCode::BAD_GATEWAY)
        );
        let (code, status, _) = classify_provider_status(403).expect("mapped");
        assert_eq!(
            (code, status),
            ("provider_auth_failed", StatusCode::BAD_GATEWAY)
        );
        let (code, status, _) = classify_provider_status(404).expect("mapped");
        assert_eq!((code, status), ("model_invalid", StatusCode::BAD_REQUEST));
        let (code, status, _) = classify_provider_status(413).expect("mapped");
        assert_eq!(
            (code, status),
            ("audio_too_large", StatusCode::PAYLOAD_TOO_LARGE)
        );
        for code in [429, 500, 502, 599] {
            let (error, status, _) = classify_provider_status(code).expect("mapped");
            assert_eq!(
                (error, status),
                ("provider_unreachable", StatusCode::BAD_GATEWAY)
            );
        }
        let (code, status, _) = classify_provider_status(400).expect("mapped");
        assert_eq!(
            (code, status),
            ("transcription_failed", StatusCode::BAD_GATEWAY)
        );
        let (code, status, _) = classify_provider_status(422).expect("mapped");
        assert_eq!(
            (code, status),
            ("transcription_failed", StatusCode::BAD_GATEWAY)
        );
        assert_eq!(classify_provider_status(301), None);
    }

    #[test]
    fn extract_text_matches_shapes() {
        // Plain-text formats return the raw body.
        for format in ["text", "srt", "vtt"] {
            assert_eq!(
                extract_text_value(b"hello there", format),
                Value::String("hello there".to_string())
            );
        }
        // JSON object with a text field.
        assert_eq!(
            extract_text_value(br#"{"text":"hi"}"#, "json"),
            Value::String("hi".to_string())
        );
        // Falsy text collapses to "" (`or ""`); truthy non-strings stay.
        assert_eq!(
            extract_text_value(br#"{"text":null}"#, "json"),
            Value::String(String::new())
        );
        assert_eq!(
            extract_text_value(br#"{}"#, "json"),
            Value::String(String::new())
        );
        assert_eq!(
            extract_text_value(br#"{"text":0}"#, "json"),
            Value::String(String::new())
        );
        assert_eq!(
            extract_text_value(br#"{"text":123}"#, "json"),
            Value::Number(123.into())
        );
        // Non-object JSON and invalid JSON fall back per the Python.
        assert_eq!(
            extract_text_value(br#"[1]"#, "json"),
            Value::String(String::new())
        );
        assert_eq!(
            extract_text_value(b"raw transcript", "json"),
            Value::String("raw transcript".to_string())
        );
        assert_eq!(
            extract_text_value(b"raw transcript", ""),
            Value::String("raw transcript".to_string())
        );
    }

    #[test]
    fn provider_url_strips_trailing_slashes() {
        assert_eq!(
            provider_transcriptions_url("https://x/v1///"),
            "https://x/v1/audio/transcriptions"
        );
    }

    #[test]
    fn optional_fields_trim_and_default() {
        let text = |name: &str, value: &str| multipart::Part {
            name: name.to_string(),
            filename: None,
            content_type: None,
            body: value.as_bytes().to_vec(),
        };
        let parts = vec![text("language", "  en  "), text("response_format", " ")];
        assert_eq!(
            optional_text_field(&parts, "language").as_deref(),
            Some("en")
        );
        assert_eq!(optional_text_field(&parts, "response_format"), None);
        assert_eq!(optional_text_field(&parts, "missing"), None);
    }
}
