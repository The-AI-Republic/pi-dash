//! D-33 external-integration shapes (stage 5, PIDASHCONV-454).
//!
//! Ports the pure half of `apps/api/pi_dash/app/views/external/base.py`
//! (243 lines): the `SUPPORTED_PROVIDERS` table (`:42-73`), the
//! `get_llm_config` branches (`:76-120`), the `get_llm_response` request
//! shape (`:123-145`), and the Unsplash URL builders (`:234-238`).
//! The HTTP shell (routes, session auth, gates, config-row SQL, the
//! outbound OpenAI/`requests` calls) lives in
//! [`crate::app_integrations::handlers_external`]; the wire behavior is
//! what is ported — no live calls happen in these unit tests.
//!
//! Fixture id: FX-EXT-01
//! (`rust-api/fixtures/app_integrations/fx-ext-01-llm.json`).
//!
//! Ported bugs (translate, don't redesign):
//!
//! * B3 (`base.py:235`): the Unsplash search URL renders `page=${page}`
//!   with a stray `$`; [`unsplash_search_url`] keeps it byte-for-byte.
//! * B5 (`base.py:159,163`): `task`/`prompt` default to `False`, not
//!   `""`. A falsy `task` answers 400; a falsy `prompt` flows into
//!   `get_llm_response` as `False`, so `task + "\n" + prompt` raises
//!   `TypeError`, which the `except Exception` swallows — the caller
//!   then maps `(None, error)` to the single generic 500. [`PromptArg`]
//!   models that: only a non-empty string is attempted, everything else
//!   forces the 500 path. The LLM error detail itself is swallowed too:
//!   every failure answers `{"error": "An internal error has occurred."}`.

use serde_json::Value;

/// Exact bytes of the config-incomplete 400 (`base.py:153-157`).
pub const LLM_CONFIG_REQUIRED_BODY: &str =
    r#"{"error":"LLM provider API key and model are required"}"#;
/// Exact bytes of the falsy-task 400 (`base.py:159-161`).
pub const TASK_REQUIRED_BODY: &str = r#"{"error":"Task is required"}"#;
/// Exact bytes of the swallowed LLM-error 500 (`base.py:164-168`).
pub const LLM_INTERNAL_ERROR_BODY: &str = r#"{"error":"An internal error has occurred."}"#;

// ---------------------------------------------------------------------------
// provider table (`base.py:42-73`)
// ---------------------------------------------------------------------------

/// One `LLMProvider` subclass: display name, supported models, default.
pub struct Provider {
    /// `cls.name` (`base.py:33-36`).
    pub name: &'static str,
    /// `cls.models`, in source order.
    pub models: &'static [&'static str],
    /// `cls.default_model`.
    pub default_model: &'static str,
}

/// `OpenAIProvider` (`base.py:42-46`).
pub const OPENAI: Provider = Provider {
    name: "OpenAI",
    models: &[
        "gpt-3.5-turbo",
        "gpt-4o-mini",
        "gpt-4o",
        "o1-mini",
        "o1-preview",
    ],
    default_model: "gpt-4o-mini",
};

/// `AnthropicProvider` (`base.py:49-62`).
pub const ANTHROPIC: Provider = Provider {
    name: "Anthropic",
    models: &[
        "claude-3-5-sonnet-20240620",
        "claude-3-haiku-20240307",
        "claude-3-opus-20240229",
        "claude-3-sonnet-20240229",
        "claude-2.1",
        "claude-2",
        "claude-instant-1.2",
        "claude-instant-1",
    ],
    default_model: "claude-3-sonnet-20240229",
};

/// `GeminiProvider` (`base.py:65-68`).
pub const GEMINI: Provider = Provider {
    name: "Gemini",
    models: &["gemini-pro", "gemini-1.5-pro-latest", "gemini-pro-vision"],
    default_model: "gemini-pro",
};

/// `SUPPORTED_PROVIDERS` (`base.py:69-73`): lookup key → provider.
/// Only the lookup is lowercased (`base.py:98`); the configured
/// `provider_key` is returned as-is (`base.py:120`).
pub const SUPPORTED_PROVIDERS: &[(&str, &Provider)] = &[
    ("openai", &OPENAI),
    ("anthropic", &ANTHROPIC),
    ("gemini", &GEMINI),
];

/// `SUPPORTED_PROVIDERS.get(provider_key.lower())` (`base.py:98`).
/// Returns the canonical lookup key and the provider.
pub fn lookup_provider(provider_key: &str) -> Option<(&'static str, &'static Provider)> {
    let lowered = provider_key.to_lowercase();
    SUPPORTED_PROVIDERS
        .iter()
        .find(|(key, _)| *key == lowered)
        .map(|(key, provider)| (*key, *provider))
}

// ---------------------------------------------------------------------------
// get_llm_config branches (`base.py:76-120`)
// ---------------------------------------------------------------------------

/// Why `get_llm_config` returned `(None, None, None)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFailure {
    /// `Unsupported provider: ...` (`base.py:98-101`).
    UnsupportedProvider,
    /// `Missing API key for provider: ...` (`base.py:103-105`).
    MissingApiKey,
    /// `Model ... not supported by ...` (`base.py:112-118`).
    UnsupportedModel,
}

/// Mirror of `get_llm_config` (`base.py:76-120`).
///
/// Inputs are the already-resolved configuration values (DB row wins,
/// else the `os.environ` default — the caller owns that read, mirroring
/// `get_configuration_value` with the `base.py:81-96` defaults).
/// Returns `(api_key, model, provider_key)` with the provider key
/// as-configured on success (`base.py:120`).
///
/// Falsy notes: `if not api_key` and `if not model` treat `""` like
/// `None` (Python truthiness); an empty model falls back to the
/// provider default (`base.py:108-109`). A `None` provider key would
/// raise `AttributeError` on `.lower()` in Python (unguarded,
/// `base.py:98`) — here it maps to [`ConfigFailure::UnsupportedProvider`]
/// and the handler's 400, since the configured default (`"openai"`)
/// means `None` is unreachable outside a NULL DB row.
pub fn resolve_llm_config(
    api_key: Option<&str>,
    provider_key: Option<&str>,
    model: Option<&str>,
) -> Result<(String, String, String), ConfigFailure> {
    let provider_key = provider_key.unwrap_or("");
    let (lookup, provider) =
        lookup_provider(provider_key).ok_or(ConfigFailure::UnsupportedProvider)?;
    let _ = lookup;
    let api_key = api_key.unwrap_or("");
    if api_key.is_empty() {
        return Err(ConfigFailure::MissingApiKey);
    }
    let model = model.unwrap_or("");
    let model = if model.is_empty() {
        provider.default_model.to_owned()
    } else {
        model.to_owned()
    };
    if !provider.models.contains(&model.as_str()) {
        return Err(ConfigFailure::UnsupportedModel);
    }
    Ok((api_key.to_owned(), model, provider_key.to_owned()))
}

// ---------------------------------------------------------------------------
// task / prompt falsy logic (B5, `base.py:159-163`)
// ---------------------------------------------------------------------------

/// A parsed `task` field: only strings survive `request.data.get`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskArg {
    /// Missing or falsy (`False`, `""`, `null`, `0`, `[]`, `{}`) → 400.
    Missing,
    /// Non-empty string → attempted.
    Text(String),
    /// Truthy non-string (`5`, `true`, `["x"]`) → passes the `if not
    /// task` guard but raises `TypeError` in `task + "\n" + prompt`,
    /// which the `except Exception` swallows → 500.
    NonStringTruthy,
}

/// A parsed `prompt` field (`request.data.get("prompt", False)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptArg {
    /// Missing or falsy → reaches `get_llm_response` as falsy →
    /// `TypeError` → swallowed → 500 (B5).
    Falsy,
    /// Non-empty string → attempted.
    Text(String),
    /// Truthy non-string → `TypeError` in the concatenation → 500.
    NonStringTruthy,
}

fn is_json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else {
                n.as_f64().is_some_and(|f| f != 0.0)
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `request.data.get("task", False)` + `if not task` (`base.py:159`).
pub fn parse_task(body: Option<&Value>) -> TaskArg {
    match body {
        None => TaskArg::Missing,
        Some(Value::String(s)) if !s.is_empty() => TaskArg::Text(s.clone()),
        Some(v) if !is_json_truthy(v) => TaskArg::Missing,
        Some(_) => TaskArg::NonStringTruthy,
    }
}

/// `request.data.get("prompt", False)` (`base.py:163`).
pub fn parse_prompt(body: Option<&Value>) -> PromptArg {
    match body {
        None => PromptArg::Falsy,
        Some(Value::String(s)) if !s.is_empty() => PromptArg::Text(s.clone()),
        Some(v) if !is_json_truthy(v) => PromptArg::Falsy,
        Some(_) => PromptArg::NonStringTruthy,
    }
}

// ---------------------------------------------------------------------------
// get_llm_response shape (`base.py:123-145`)
// ---------------------------------------------------------------------------

/// `final_text = task + "\n" + prompt` (`base.py:125`).
pub fn final_text(task: &str, prompt: &str) -> String {
    format!("{task}\n{prompt}")
}

/// Gemini quirk: `model = f"gemini/{model}"`, still sent through the
/// OpenAI client (`base.py:128-129`).
pub fn rewrite_model(provider_key: &str, model: &str) -> String {
    if provider_key.to_lowercase() == "gemini" {
        format!("gemini/{model}")
    } else {
        model.to_owned()
    }
}

/// `text.replace("\n", "<br/>")` (`base.py:174,209`).
pub fn response_html(text: &str) -> String {
    text.replace('\n', "<br/>")
}

// ---------------------------------------------------------------------------
// Unsplash URL builders (`base.py:234-238`)
// ---------------------------------------------------------------------------

/// Search URL (`base.py:235`) — B3 kept: `page=${page}` with the stray
/// `$` is ported byte-for-byte.
pub fn unsplash_search_url(access_key: &str, query: &str, page: &str, per_page: &str) -> String {
    format!(
        "https://api.unsplash.com/search/photos/?client_id={access_key}&query={query}&page=${page}&per_page={per_page}"
    )
}

/// List URL (`base.py:237`).
pub fn unsplash_list_url(access_key: &str, page: &str, per_page: &str) -> String {
    format!(
        "https://api.unsplash.com/photos/?client_id={access_key}&page={page}&per_page={per_page}"
    )
}

/// `query = request.GET.get("query", False)` — only a non-empty query
/// takes the search shape; `page`/`per_page` default to `"1"`/`"20"`
/// as raw strings with no int coercion (`base.py:230-232`).
pub fn unsplash_url(
    access_key: &str,
    query: Option<&str>,
    page: Option<&str>,
    per_page: Option<&str>,
) -> String {
    let page = page.unwrap_or("1");
    let per_page = per_page.unwrap_or("20");
    match query {
        Some(q) if !q.is_empty() => unsplash_search_url(access_key, q, page, per_page),
        _ => unsplash_list_url(access_key, page, per_page),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> serde_json::Value {
        let path = format!(
            "{}/../../fixtures/app_integrations/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    // -- FX-EXT-01: provider table ------------------------------------------

    #[test]
    fn provider_table_matches_fixture() {
        let gold = fixture("fx-ext-01-llm.json");
        let providers = &gold["providers"];
        for (key, table) in SUPPORTED_PROVIDERS.iter() {
            let entry = &providers[key];
            assert_eq!(entry["name"], table.name, "name for {key}");
            assert_eq!(
                entry["default_model"], table.default_model,
                "default for {key}"
            );
            let models: Vec<&str> = table.models.to_vec();
            let gold_models: Vec<&str> = entry["models"]
                .as_array()
                .expect("models array")
                .iter()
                .map(|v| v.as_str().expect("model string"))
                .collect();
            assert_eq!(models, gold_models, "models for {key}");
        }
        assert_eq!(SUPPORTED_PROVIDERS.len(), 3, "openai + anthropic + gemini");
    }

    #[test]
    fn provider_lookup_is_case_insensitive_but_key_preserved() {
        // `SUPPORTED_PROVIDERS.get(provider_key.lower())` (fixture quirk).
        let (key, provider) = lookup_provider("OpenAI").expect("openai resolves");
        assert_eq!(key, "openai");
        assert_eq!(provider.name, "OpenAI");
        assert!(lookup_provider("NOPE").is_none());
        // Success returns the key as-configured (base.py:120).
        let (api_key, model, provider_key) =
            resolve_llm_config(Some("k"), Some("OpenAI"), Some("gpt-4o")).expect("valid triple");
        assert_eq!(
            (api_key.as_str(), model.as_str(), provider_key.as_str()),
            ("k", "gpt-4o", "OpenAI")
        );
    }

    // -- FX-EXT-01: get_llm_config branches ----------------------------------

    #[test]
    fn config_ok_and_model_default() {
        // Empty model falls back to the provider default (base.py:108-109).
        let (api_key, model, provider) =
            resolve_llm_config(Some("k"), Some("openai"), None).expect("defaults to gpt-4o-mini");
        assert_eq!(
            (api_key.as_str(), model.as_str(), provider.as_str()),
            ("k", "gpt-4o-mini", "openai")
        );
        let (_, model, _) =
            resolve_llm_config(Some("k"), Some("gemini"), Some("")).expect("empty model defaults");
        assert_eq!(model, "gemini-pro");
    }

    #[test]
    fn config_failures_match_fixture_branches() {
        assert_eq!(
            resolve_llm_config(Some("k"), Some("nope"), Some("gpt-4o")),
            Err(ConfigFailure::UnsupportedProvider)
        );
        assert_eq!(
            resolve_llm_config(None, Some("openai"), Some("gpt-4o")),
            Err(ConfigFailure::MissingApiKey)
        );
        assert_eq!(
            resolve_llm_config(Some(""), Some("openai"), Some("gpt-4o")),
            Err(ConfigFailure::MissingApiKey)
        );
        assert_eq!(
            resolve_llm_config(Some("k"), Some("openai"), Some("claude-2")),
            Err(ConfigFailure::UnsupportedModel)
        );
    }

    // -- FX-EXT-01: task / prompt falsy logic (B5) ----------------------------

    #[test]
    fn missing_or_falsy_task_is_400() {
        assert_eq!(parse_task(None), TaskArg::Missing);
        assert_eq!(parse_task(Some(&serde_json::json!(""))), TaskArg::Missing);
        assert_eq!(
            parse_task(Some(&serde_json::json!(false))),
            TaskArg::Missing
        );
        assert_eq!(parse_task(Some(&serde_json::json!(null))), TaskArg::Missing);
        assert_eq!(parse_task(Some(&serde_json::json!(0))), TaskArg::Missing);
        assert_eq!(
            parse_task(Some(&serde_json::json!("summarize"))),
            TaskArg::Text("summarize".to_owned())
        );
        // Truthy non-string passes the guard but blows up in the
        // concatenation → 500 path.
        assert_eq!(
            parse_task(Some(&serde_json::json!(5))),
            TaskArg::NonStringTruthy
        );
    }

    #[test]
    fn falsy_prompt_forces_500_path() {
        assert_eq!(parse_prompt(None), PromptArg::Falsy);
        assert_eq!(parse_prompt(Some(&serde_json::json!(""))), PromptArg::Falsy);
        assert_eq!(
            parse_prompt(Some(&serde_json::json!(false))),
            PromptArg::Falsy
        );
        assert_eq!(
            parse_prompt(Some(&serde_json::json!("hello"))),
            PromptArg::Text("hello".to_owned())
        );
        assert_eq!(
            parse_prompt(Some(&serde_json::json!(5))),
            PromptArg::NonStringTruthy
        );
    }

    // -- FX-EXT-01: get_llm_response shape ------------------------------------

    #[test]
    fn final_text_and_gemini_rewrite() {
        assert_eq!(final_text("task", "prompt"), "task\nprompt");
        assert_eq!(rewrite_model("gemini", "gemini-pro"), "gemini/gemini-pro");
        assert_eq!(rewrite_model("Gemini", "gemini-pro"), "gemini/gemini-pro");
        assert_eq!(rewrite_model("openai", "gpt-4o"), "gpt-4o");
    }

    #[test]
    fn response_html_replaces_newlines() {
        assert_eq!(response_html("a\nb\nc"), "a<br/>b<br/>c");
        assert_eq!(response_html("flat"), "flat");
    }

    // -- FX-EXT-01: unsplash shapes (incl B3) ----------------------------------

    #[test]
    fn unsplash_urls_match_fixture_shapes() {
        let gold = fixture("fx-ext-01-llm.json");
        let shape = gold["endpoints"]["unsplash"]["url_shape"]
            .as_str()
            .expect("shape");
        assert!(shape.contains("page=$"), "fixture pins the B3 stray $");
        assert_eq!(
            unsplash_search_url("KEY", "cats", "2", "5"),
            "https://api.unsplash.com/search/photos/?client_id=KEY&query=cats&page=$2&per_page=5"
        );
        assert_eq!(
            unsplash_list_url("KEY", "2", "5"),
            "https://api.unsplash.com/photos/?client_id=KEY&page=2&per_page=5"
        );
    }

    #[test]
    fn unsplash_url_dispatch_and_raw_defaults() {
        // Missing/empty query takes the list shape; page/per_page stay
        // raw strings with "1"/"20" defaults (base.py:230-232).
        assert_eq!(
            unsplash_url("K", None, None, None),
            "https://api.unsplash.com/photos/?client_id=K&page=1&per_page=20"
        );
        assert_eq!(
            unsplash_url("K", Some(""), None, None),
            "https://api.unsplash.com/photos/?client_id=K&page=1&per_page=20"
        );
        assert!(unsplash_url("K", Some("cats"), Some("2"), Some("5")).contains("/search/photos/"));
    }

    #[test]
    fn error_bodies_are_byte_exact() {
        // Error strings pinned against the FX-EXT-01 endpoint rows.
        let gold = fixture("fx-ext-01-llm.json");
        let errors = gold["endpoints"]["project_gpt"]["errors"]
            .as_array()
            .expect("errors");
        let texts: Vec<&str> = errors
            .iter()
            .map(|e| e["error"].as_str().expect("string"))
            .collect();
        assert!(texts.contains(&"Task is required"));
        assert!(texts.contains(&"An internal error has occurred."));
        assert!(texts.contains(&"LLM provider API key and model are required"));
        assert_eq!(
            LLM_CONFIG_REQUIRED_BODY,
            r#"{"error":"LLM provider API key and model are required"}"#
        );
        assert_eq!(TASK_REQUIRED_BODY, r#"{"error":"Task is required"}"#);
        assert_eq!(
            LLM_INTERNAL_ERROR_BODY,
            r#"{"error":"An internal error has occurred."}"#
        );
    }
}
