//! Assistant serializer DTOs + pure validators (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/serializers.py:1-162`:
//!
//! * `AssistantThreadSerializer` (`:20-29`) → [`ThreadView`] +
//!   [`has_active_turn`]. There is intentionally no message serializer:
//!   the message wire format is produced by `events.message_envelope`, so
//!   the list endpoint and the SSE stream emit byte-identical shapes
//!   (`:32-34`).
//! * `UserLLMConfigSerializer` (`:37-75`) → [`LlmConfigView`] +
//!   [`validate_model_name`], [`validate_base_url`], [`validate_api_key`],
//!   [`validate_llm_config`].
//! * `UserSTTConfigSerializer` (`:78-120`) → [`SttConfigView`] + the shared
//!   field validators + [`validate_stt_config`]. Dictation has a single
//!   OpenAI-compatible provider, so there is no `provider_kind` field and
//!   `base_url` is always required.
//! * `AssistantMCPServerSerializer` (`:123-162`) → [`McpServerView`] +
//!   [`validate_mcp_name`], [`validate_mcp_url`].
//!
//! Layering notes (license-domain precedent):
//!
//! * These are output shapes plus the pure validation rules. Struct field
//!   declaration order is the `Meta.fields` order minus the write-only keys
//!   (`api_key`, `auth_header`), which serde then emits in declaration
//!   order; write-only values are accepted on input and never rendered back
//!   — the views below have no such field, so absence is structural.
//! * Datetimes cross this boundary already rendered as DRF `iso-8601`
//!   strings and ids as UUID strings (borrowed `&str`, like the license
//!   views); rendering owns to the handlers layer.
//! * `has_api_key` (`models.py:261,292`), `has_auth_header` and
//!   `tool_prefix` (`models.py:222-233`) are model properties computed with
//!   the models layer (PIDASHCONV-247); here they are plain `bool` / `str`
//!   output fields.
//! * `Meta.read_only_fields` / `write_only` / `required` / `allow_blank` /
//!   `max_length` metadata is recorded as constants for the handlers layer,
//!   which owns request parsing. Field-level `max_length` / requiredness
//!   enforcement is not re-implemented here: the fixtures pin no
//!   `max_length` error cases, and DRF's auto-validators belong with the
//!   request edge, not the pure layer.
//! * URL checks replicate `urllib.parse.urlparse` semantics exactly (probed
//!   against the live stdlib): the scheme must match
//!   `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )` and compare
//!   case-insensitively to `http`/`https`; credentials reject only on a
//!   non-empty username or non-empty password, so `http://@host` and
//!   `http://:@host` pass exactly as in Python.
//!
//! Fixture ids replayed by the unit tests alongside this module:
//! `rust-api/fixtures/assistant/serializers/*.golden.json` (F-A6-01).

use serde::Serialize;

/// Known provider kinds (`models.py:179-181`). The `ProviderKind` enum
/// itself belongs to the models layer (F-A6-03); the serializer rules only
/// need the wire strings and the default.
pub const PROVIDER_OPENAI_COMPATIBLE: &str = "openai_compatible";
/// Known provider kinds (`models.py:179-181`).
pub const PROVIDER_ANTHROPIC: &str = "anthropic";

/// `AssistantThread.title` column width (`models.py:36`).
pub const THREAD_TITLE_MAX_LENGTH: usize = 255;
/// `base_url` column width (`models.py:247,278`).
pub const BASE_URL_MAX_LENGTH: usize = 500;
/// `model_name` column width (`models.py:248,279`).
pub const MODEL_NAME_MAX_LENGTH: usize = 255;
/// `api_key` write-only input cap (`serializers.py:38,86`).
pub const API_KEY_MAX_LENGTH: usize = 512;
/// `AssistantMCPServer.name` column width (`models.py:204`).
pub const MCP_NAME_MAX_LENGTH: usize = 80;
/// `AssistantMCPServer.url` column width (`models.py:205`).
pub const MCP_URL_MAX_LENGTH: usize = 500;
/// `auth_header` write-only input cap (`serializers.py:124-126`).
pub const AUTH_HEADER_MAX_LENGTH: usize = 2048;

/// `validate_model_name` failure (`serializers.py:49,97`).
pub const MODEL_NAME_REQUIRED: &str = "A model name is required.";
/// LLM/STT `validate_base_url` scheme failure (`serializers.py:58,106`).
pub const BASE_URL_HTTP: &str = "base_url must be an http(s) URL.";
/// LLM/STT `validate_base_url` credentials failure (`serializers.py:60,108`).
pub const BASE_URL_NO_CREDENTIALS: &str = "base_url must not contain credentials.";
/// `validate_api_key` short-key failure (`serializers.py:65,113`).
pub const API_KEY_TOO_SHORT: &str = "API key looks too short.";
/// `UserLLMConfigSerializer.validate` cross-field failure
/// (`serializers.py:72-74`).
pub const LLM_BASE_URL_REQUIRED: &str = "base_url is required for OpenAI-compatible providers.";
/// `UserSTTConfigSerializer.validate` cross-field failure
/// (`serializers.py:119`).
pub const STT_BASE_URL_REQUIRED: &str = "base_url is required.";
/// `validate_name` failure (`serializers.py:148`).
pub const MCP_NAME_REQUIRED: &str = "A name is required.";
/// `validate_url` empty failure (`serializers.py:154`).
pub const MCP_URL_REQUIRED: &str = "A server URL is required.";
/// `validate_url` scheme failure (`serializers.py:157`).
pub const MCP_URL_HTTP: &str = "url must be an http(s) URL.";
/// `validate_url` credentials failure (`serializers.py:161`).
pub const MCP_URL_NO_CREDENTIALS: &str = "url must not contain credentials.";

/// `AssistantThreadSerializer.Meta.fields` order (`serializers.py:25`).
pub const THREAD_FIELDS: [&str; 6] = [
    "id",
    "title",
    "is_archived",
    "has_active_turn",
    "created_at",
    "updated_at",
];
/// `AssistantThreadSerializer.Meta.read_only_fields` (`serializers.py:26`).
pub const THREAD_READ_ONLY_FIELDS: [&str; 4] =
    ["id", "created_at", "updated_at", "has_active_turn"];
/// `UserLLMConfigSerializer.Meta.fields` order (`serializers.py:43`).
pub const LLM_CONFIG_FIELDS: [&str; 6] = [
    "provider_kind",
    "base_url",
    "model_name",
    "api_key",
    "has_api_key",
    "last_verified_at",
];
/// `UserLLMConfigSerializer.Meta.read_only_fields` (`serializers.py:44`).
pub const LLM_CONFIG_READ_ONLY_FIELDS: [&str; 2] = ["has_api_key", "last_verified_at"];
/// The write-only encrypted input (`serializers.py:38`): accepted on write,
/// never rendered back.
pub const LLM_CONFIG_WRITE_ONLY_FIELDS: [&str; 1] = ["api_key"];
/// `UserSTTConfigSerializer.Meta.fields` order (`serializers.py:91`).
pub const STT_CONFIG_FIELDS: [&str; 5] = [
    "base_url",
    "model_name",
    "api_key",
    "has_api_key",
    "last_verified_at",
];
/// `UserSTTConfigSerializer.Meta.read_only_fields` (`serializers.py:92`).
pub const STT_CONFIG_READ_ONLY_FIELDS: [&str; 2] = ["has_api_key", "last_verified_at"];
/// The write-only encrypted input (`serializers.py:86`).
pub const STT_CONFIG_WRITE_ONLY_FIELDS: [&str; 1] = ["api_key"];
/// `AssistantMCPServerSerializer.Meta.fields` order (`serializers.py:132-142`).
pub const MCP_SERVER_FIELDS: [&str; 9] = [
    "id",
    "name",
    "url",
    "auth_header",
    "has_auth_header",
    "tool_prefix",
    "is_enabled",
    "created_at",
    "updated_at",
];
/// `AssistantMCPServerSerializer.Meta.read_only_fields`
/// (`serializers.py:143`).
pub const MCP_SERVER_READ_ONLY_FIELDS: [&str; 5] = [
    "id",
    "has_auth_header",
    "tool_prefix",
    "created_at",
    "updated_at",
];
/// The write-only encrypted input (`serializers.py:124-126`).
pub const MCP_SERVER_WRITE_ONLY_FIELDS: [&str; 1] = ["auth_header"];

/// `AssistantThreadSerializer.to_representation` output
/// (`serializers.py:20-29`): `Meta.fields` order; no message keys (there is
/// no message serializer by design, `:32-34`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThreadView<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub is_archived: bool,
    pub has_active_turn: bool,
    pub created_at: &'a str,
    pub updated_at: &'a str,
}

/// `UserLLMConfigSerializer` read output (`serializers.py:37-44`): the
/// `Meta.fields` order minus the write-only `api_key`, which never appears
/// in output — the API only reports `has_api_key`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LlmConfigView<'a> {
    pub provider_kind: &'a str,
    pub base_url: &'a str,
    pub model_name: &'a str,
    pub has_api_key: bool,
    pub last_verified_at: Option<&'a str>,
}

/// `UserSTTConfigSerializer` read output (`serializers.py:78-92`): no
/// `provider_kind` field; the write-only `api_key` never appears.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SttConfigView<'a> {
    pub base_url: &'a str,
    pub model_name: &'a str,
    pub has_api_key: bool,
    pub last_verified_at: Option<&'a str>,
}

/// `AssistantMCPServerSerializer` read output (`serializers.py:123-143`):
/// the `Meta.fields` order minus the write-only `auth_header`; `tool_prefix`
/// is a read-only slug (`:128`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct McpServerView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub url: &'a str,
    pub has_auth_header: bool,
    pub tool_prefix: &'a str,
    pub is_enabled: bool,
    pub created_at: &'a str,
    pub updated_at: &'a str,
}

/// `get_has_active_turn` (`serializers.py:28-29`):
/// `obj.active_turn_id is not None`.
pub fn has_active_turn<T>(active_turn_id: Option<T>) -> bool {
    active_turn_id.is_some()
}

/// `validate_model_name` (`serializers.py:46-50,94-98`): strip; a blank
/// value is rejected. Shared verbatim by the LLM and STT serializers.
pub fn validate_model_name(value: &str) -> Result<String, &'static str> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        return Err(MODEL_NAME_REQUIRED);
    }
    Ok(normalized)
}

/// Scheme per `urllib.parse.urlparse`: leading `ALPHA *( ALPHA / DIGIT /
/// "+" / "-" / "." )` followed by `":"`, lowercased for comparison
/// (urlparse lowercases the scheme; `HTTPS://…` is accepted).
fn url_scheme(value: &str) -> Option<String> {
    let colon = value.find(':')?;
    let scheme = &value[..colon];
    let mut chars = scheme.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return None,
    }
    if !scheme
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
    {
        return None;
    }
    Some(scheme.to_ascii_lowercase())
}

/// Whether the URL carries credentials (`parsed.username or
/// parsed.password`, `serializers.py:59-60,107-108,158-161`).
///
/// Only the authority (after `scheme://`, up to the next `/`, `?` or `#`)
/// counts, and rejection needs a *non-empty* username or a *non-empty*
/// password — `http://@host` and `http://:@host` pass exactly as in Python,
/// where both are falsy.
fn url_has_credentials(value: &str, scheme_end: usize) -> bool {
    let rest = &value[scheme_end..];
    let authority = rest
        .strip_prefix("//")
        .map(|a| a.split(['/', '?', '#']).next().unwrap_or(""))
        .unwrap_or("");
    let Some(at) = authority.rfind('@') else {
        return false;
    };
    let userinfo = &authority[..at];
    match userinfo.find(':') {
        Some(i) => !userinfo[..i].is_empty() || !userinfo[i + 1..].is_empty(),
        None => !userinfo.is_empty(),
    }
}

/// Shared URL normalizer behind `validate_base_url` and `validate_mcp_url`:
/// strip whitespace, strip trailing slashes, require an `http(s)` scheme
/// with no credentials. `scheme_error` / `credentials_error` carry the
/// field-specific messages; `empty_err` is the field-specific empty outcome
/// (`None` → `Ok("")` passthrough for `base_url`, `Some(…required)` for MCP
/// `url`). All three spell DRF field-level `ValidationError("msg")`
/// (list-form); only the cross-field `validate_*` fns return tuples.
fn normalize_http_url(
    value: &str,
    scheme_error: &'static str,
    credentials_error: &'static str,
    empty_err: Option<&'static str>,
) -> Result<String, &'static str> {
    let normalized: String = value.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        return match empty_err {
            Some(msg) => Err(msg),
            None => Ok(String::new()),
        };
    }
    let Some(scheme) = url_scheme(&normalized) else {
        return Err(scheme_error);
    };
    if scheme != "http" && scheme != "https" {
        return Err(scheme_error);
    }
    let scheme_end = normalized.find(':').map(|i| i + 1).unwrap_or(0);
    if url_has_credentials(&normalized, scheme_end) {
        return Err(credentials_error);
    }
    Ok(normalized)
}

/// `UserLLMConfigSerializer.validate_base_url`
/// (`serializers.py:52-61`): an empty value passes through; otherwise the
/// URL must be `http(s)` without credentials.
pub fn validate_base_url(value: &str) -> Result<String, &'static str> {
    normalize_http_url(value, BASE_URL_HTTP, BASE_URL_NO_CREDENTIALS, None)
}

/// `UserLLMConfigSerializer.validate_api_key`
/// (`serializers.py:63-66`, shared verbatim by STT `:111-114`): only a
/// non-empty value shorter than 8 characters is rejected. The length is in
/// code points (`chars().count()`), matching Python `len(value)` — byte
/// length would diverge on non-ASCII input (semantic trap).
pub fn validate_api_key(value: &str) -> Result<String, &'static str> {
    if !value.is_empty() && value.chars().count() < 8 {
        return Err(API_KEY_TOO_SHORT);
    }
    Ok(value.to_string())
}

/// Cross-field input for `UserLLMConfigSerializer.validate`
/// (`serializers.py:68-75`): the supplied attributes; `None` means the
/// field was omitted from the request.
pub struct LlmConfigAttrs<'a> {
    pub provider_kind: Option<&'a str>,
    pub base_url: Option<&'a str>,
}

/// Stored row values backing the `or` / `.get` fallbacks in
/// `UserLLMConfigSerializer.validate`. `None` is "no instance" (create):
/// the provider defaults to `openai_compatible` (`serializers.py:69`) and
/// the URL to `""` (`:70`).
pub struct LlmConfigInstance<'a> {
    pub provider_kind: &'a str,
    pub base_url: &'a str,
}

/// `UserLLMConfigSerializer.validate` (`serializers.py:68-75`):
/// `provider_kind` falls back to the instance (falsy attrs value included —
/// Python `or`), while `base_url` uses get-with-default (an explicitly
/// supplied `""` stays). An OpenAI-compatible provider with an empty URL
/// fails on `base_url`.
pub fn validate_llm_config(
    attrs: &LlmConfigAttrs<'_>,
    instance: Option<&LlmConfigInstance<'_>>,
) -> Result<(), (&'static str, &'static str)> {
    let provider = match attrs.provider_kind {
        Some(kind) if !kind.is_empty() => kind,
        _ => instance
            .map(|i| i.provider_kind)
            .unwrap_or(PROVIDER_OPENAI_COMPATIBLE),
    };
    let base_url = match attrs.base_url {
        Some(url) => url,
        None => instance.map(|i| i.base_url).unwrap_or(""),
    };
    if provider == PROVIDER_OPENAI_COMPATIBLE && base_url.is_empty() {
        return Err(("base_url", LLM_BASE_URL_REQUIRED));
    }
    Ok(())
}

/// `UserSTTConfigSerializer.validate` (`serializers.py:116-120`): `base_url`
/// falls back to the instance URL and is always required (no provider
/// selector to condition on).
pub fn validate_stt_config(
    base_url: Option<&str>,
    instance_base_url: Option<&str>,
) -> Result<(), (&'static str, &'static str)> {
    let url = base_url.or(instance_base_url).unwrap_or("");
    if url.is_empty() {
        return Err(("base_url", STT_BASE_URL_REQUIRED));
    }
    Ok(())
}

/// `AssistantMCPServerSerializer.validate_name`
/// (`serializers.py:145-149`): strip; a blank name is rejected.
pub fn validate_mcp_name(value: &str) -> Result<String, &'static str> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        return Err(MCP_NAME_REQUIRED);
    }
    Ok(normalized)
}

/// `AssistantMCPServerSerializer.validate_url`
/// (`serializers.py:151-162`): strip, strip trailing slashes, then require
/// a non-empty `http(s)` URL without credentials.
pub fn validate_mcp_url(value: &str) -> Result<String, &'static str> {
    normalize_http_url(
        value,
        MCP_URL_HTTP,
        MCP_URL_NO_CREDENTIALS,
        Some(MCP_URL_REQUIRED),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn golden(name: &str) -> Value {
        let path = format!(
            "{}/../../fixtures/assistant/serializers/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    /// Top-level JSON key order of a struct's serialization, read off the
    /// serialized bytes (struct fields emit in declaration order).
    fn serialized_keys<T: serde::Serialize>(value: &T) -> Vec<String> {
        let rendered = serde_json::to_string(value).expect("serializes");
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut chars = rendered.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                '"' if depth == 1 => {
                    let mut key = String::new();
                    for ch in chars.by_ref() {
                        if ch == '"' {
                            break;
                        }
                        key.push(ch);
                    }
                    if chars.peek() == Some(&':') {
                        chars.next();
                        keys.push(key);
                    }
                }
                _ => {}
            }
        }
        keys
    }

    #[test]
    fn thread_shape_and_has_active_turn_match_fixture() {
        let fixture = golden("thread.golden.json");
        let meta = &fixture["meta"];
        let fields: Vec<&str> = meta["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .map(|v| v.as_str().expect("field"))
            .collect();
        assert_eq!(fields, THREAD_FIELDS);
        let read_only: Vec<&str> = meta["read_only"]
            .as_array()
            .expect("read_only")
            .iter()
            .map(|v| v.as_str().expect("field"))
            .collect();
        assert_eq!(read_only, THREAD_READ_ONLY_FIELDS);

        // serializers.py:28-29 — both fixture cases.
        assert!(!has_active_turn::<&str>(None));
        assert!(has_active_turn(Some("<uuid>")));

        let view = ThreadView {
            id: "11111111-1111-4111-8111-111111111111",
            title: "T",
            is_archived: false,
            has_active_turn: has_active_turn::<&str>(None),
            created_at: "2026-09-28T00:00:00Z",
            updated_at: "2026-09-28T00:00:00Z",
        };
        assert_eq!(serialized_keys(&view), THREAD_FIELDS);
        let body = serde_json::to_value(&view).expect("value");
        assert_eq!(body["has_active_turn"], false);
    }

    #[test]
    fn llm_config_shape_hides_write_only_key() {
        let fixture = golden("llm_config.golden.json");
        let meta = &fixture["meta"];
        let fields: Vec<&str> = meta["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .map(|v| v.as_str().expect("field"))
            .collect();
        assert_eq!(fields, LLM_CONFIG_FIELDS);
        assert_eq!(meta["write_only"][0], "api_key");

        // The read output is Meta.fields minus the write-only api_key, in
        // order — api_key never appears in output.
        let view = LlmConfigView {
            provider_kind: PROVIDER_OPENAI_COMPATIBLE,
            base_url: "https://api.openai.com/v1",
            model_name: "gpt-4o",
            has_api_key: true,
            last_verified_at: None,
        };
        let rendered = serde_json::to_string(&view).expect("serializes");
        assert_eq!(
            serialized_keys(&view),
            [
                "provider_kind",
                "base_url",
                "model_name",
                "has_api_key",
                "last_verified_at"
            ]
        );
        let body: Value = serde_json::from_str(&rendered).expect("parses");
        assert!(
            body.get("api_key").is_none(),
            "write-only api_key must never render: {rendered}"
        );
        assert_eq!(body["has_api_key"], true);
        assert_eq!(body["last_verified_at"], Value::Null);
    }

    #[test]
    fn llm_config_field_validators_match_fixture_cases() {
        // model_name stripped / blank rejected (:46-50).
        assert_eq!(validate_model_name("  gpt-4o  "), Ok("gpt-4o".into()));
        assert_eq!(validate_model_name("   "), Err(MODEL_NAME_REQUIRED));
        assert_eq!(MODEL_NAME_REQUIRED, "A model name is required.");
        // base_url trailing slash stripped / empty passthrough (:52-61).
        assert_eq!(
            validate_base_url("https://api.openai.com/v1/"),
            Ok("https://api.openai.com/v1".into())
        );
        assert_eq!(validate_base_url(""), Ok(String::new()));
        assert_eq!(validate_base_url("ftp://x"), Err(BASE_URL_HTTP));
        assert_eq!(
            validate_base_url("https://user:pw@host/v1"),
            Err(BASE_URL_NO_CREDENTIALS)
        );
        // short api_key rejected (:63-66); empty passes.
        assert_eq!(validate_api_key("short"), Err(API_KEY_TOO_SHORT));
        assert_eq!(validate_api_key(""), Ok(String::new()));
        assert_eq!(
            validate_api_key("sk-ant-1234567890"),
            Ok("sk-ant-1234567890".into())
        );
        // Length is code points, not bytes (probed: len("é"*7) == 7).
        assert_eq!(validate_api_key(&"é".repeat(7)), Err(API_KEY_TOO_SHORT));
        assert_eq!(validate_api_key(&"é".repeat(8)), Ok("é".repeat(8)));
    }

    #[test]
    fn llm_config_cross_field_rule_matches_fixture() {
        // openai_compatible requires base_url (:68-75); the provider falls
        // back to the instance default openai_compatible.
        let attrs = LlmConfigAttrs {
            provider_kind: None,
            base_url: None,
        };
        assert_eq!(
            validate_llm_config(&attrs, None),
            Err(("base_url", LLM_BASE_URL_REQUIRED))
        );
        let with_url = LlmConfigAttrs {
            provider_kind: None,
            base_url: Some("https://api.openai.com/v1"),
        };
        assert_eq!(validate_llm_config(&with_url, None), Ok(()));
        // A non-default provider needs no URL; an explicitly empty attrs
        // provider_kind falls back to the instance (Python `or`).
        let anthropic = LlmConfigAttrs {
            provider_kind: Some(PROVIDER_ANTHROPIC),
            base_url: None,
        };
        assert_eq!(validate_llm_config(&anthropic, None), Ok(()));
        let instance = LlmConfigInstance {
            provider_kind: PROVIDER_ANTHROPIC,
            base_url: "",
        };
        let blank_provider = LlmConfigAttrs {
            provider_kind: Some(""),
            base_url: None,
        };
        assert_eq!(
            validate_llm_config(&blank_provider, Some(&instance)),
            Ok(())
        );
        // An explicitly supplied "" base_url stays "" (get-with-default),
        // even when the instance has one.
        let stored = LlmConfigInstance {
            provider_kind: PROVIDER_OPENAI_COMPATIBLE,
            base_url: "https://api.openai.com/v1",
        };
        let cleared = LlmConfigAttrs {
            provider_kind: None,
            base_url: Some(""),
        };
        assert_eq!(
            validate_llm_config(&cleared, Some(&stored)),
            Err(("base_url", LLM_BASE_URL_REQUIRED))
        );
    }

    #[test]
    fn stt_config_shape_and_rules_match_fixture() {
        let fixture = golden("stt_config.golden.json");
        let meta = &fixture["meta"];
        let fields: Vec<&str> = meta["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .map(|v| v.as_str().expect("field"))
            .collect();
        assert_eq!(fields, STT_CONFIG_FIELDS);
        // No provider_kind field on the STT serializer.
        assert!(!fields.contains(&"provider_kind"));
        assert_eq!(meta["write_only"][0], "api_key");

        let view = SttConfigView {
            base_url: "https://api.openai.com/v1",
            model_name: "whisper-1",
            has_api_key: false,
            last_verified_at: None,
        };
        let rendered = serde_json::to_string(&view).expect("serializes");
        assert_eq!(
            serialized_keys(&view),
            ["base_url", "model_name", "has_api_key", "last_verified_at"]
        );
        let body: Value = serde_json::from_str(&rendered).expect("parses");
        assert!(
            body.get("api_key").is_none(),
            "write-only api_key must never render: {rendered}"
        );

        // Empty attrs with no instance URL rejected (:116-120).
        assert_eq!(
            validate_stt_config(None, None),
            Err(("base_url", STT_BASE_URL_REQUIRED))
        );
        assert_eq!(validate_stt_config(Some("https://x"), None), Ok(()));
        assert_eq!(validate_stt_config(None, Some("https://x")), Ok(()));
        // Validators mirror the LLM config messages (:94-114).
        assert_eq!(validate_model_name("   "), Err(MODEL_NAME_REQUIRED));
        assert_eq!(validate_base_url("ftp://x"), Err(BASE_URL_HTTP));
        assert_eq!(validate_api_key("short"), Err(API_KEY_TOO_SHORT));
    }

    #[test]
    fn mcp_server_shape_hides_write_only_header() {
        let fixture = golden("mcp_server.golden.json");
        let meta = &fixture["meta"];
        let fields: Vec<&str> = meta["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .map(|v| v.as_str().expect("field"))
            .collect();
        assert_eq!(fields, MCP_SERVER_FIELDS);
        assert_eq!(meta["write_only"][0], "auth_header");

        let view = McpServerView {
            id: "22222222-2222-4222-8222-222222222222",
            name: "Tools",
            url: "https://mcp.example.com/sse",
            has_auth_header: true,
            tool_prefix: "mcp_tools",
            is_enabled: true,
            created_at: "2026-09-28T00:00:00Z",
            updated_at: "2026-09-28T00:00:00Z",
        };
        assert_eq!(
            serialized_keys(&view),
            [
                "id",
                "name",
                "url",
                "has_auth_header",
                "tool_prefix",
                "is_enabled",
                "created_at",
                "updated_at"
            ]
        );
        let body = serde_json::to_value(&view).expect("value");
        assert!(
            body.get("auth_header").is_none(),
            "write-only auth_header must never render: {body}"
        );
        assert_eq!(body["has_auth_header"], true);
    }

    #[test]
    fn mcp_server_field_validators_match_fixture_cases() {
        // Empty url rejected; trailing slash stripped (:151-162).
        assert_eq!(validate_mcp_url(""), Err(MCP_URL_REQUIRED));
        assert_eq!(
            validate_mcp_url("https://mcp.example.com/sse/"),
            Ok("https://mcp.example.com/sse".into())
        );
        assert_eq!(validate_mcp_url("ftp://x"), Err(MCP_URL_HTTP));
        assert_eq!(
            validate_mcp_url("https://user:pw@host/sse"),
            Err(MCP_URL_NO_CREDENTIALS)
        );
        // Blank name rejected; name stripped (:145-149).
        assert_eq!(validate_mcp_name(""), Err(MCP_NAME_REQUIRED));
        assert_eq!(validate_mcp_name("  Tools  "), Ok("Tools".into()));
    }

    #[test]
    fn url_edge_cases_match_urlparse() {
        // Probed against the live stdlib urlparse.
        assert_eq!(
            validate_base_url("HTTPS://x/v1/"),
            Ok("HTTPS://x/v1".into())
        );
        assert_eq!(validate_base_url("http:foo"), Ok("http:foo".into()));
        assert_eq!(validate_base_url("http://@host"), Ok("http://@host".into()));
        assert_eq!(
            validate_base_url("http://:@host"),
            Ok("http://:@host".into())
        );
        assert_eq!(validate_base_url("1http://x"), Err(BASE_URL_HTTP));
        assert_eq!(validate_base_url("notaurl"), Err(BASE_URL_HTTP));
        assert_eq!(validate_base_url("///"), Ok(String::new()));
        assert_eq!(validate_mcp_url("///"), Err(MCP_URL_REQUIRED));
    }
}
