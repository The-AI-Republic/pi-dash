//! Provider lane helpers shared by the chat engine's streaming lane
//! (`streaming.rs`): lane selection, wire-tool construction and the
//! OpenAI / Anthropic schema transforms (`pydantic_ai/models/
//! {anthropic,openai}.py`, pinned 1.107.0), tool-call arguments, the
//! provider error taxonomy and Python `repr` rendering.
//!
//! The non-streaming request builders and response parsers the retired
//! issue-run path used were deleted in P3 of the agent naming work; the
//! streaming lane builds its own request bodies.

use serde_json::{Map, Value};

/// `User-Agent` both lanes send (`models/__init__.py:get_user_agent`,
/// pinned pydantic-ai 1.107.0).
pub const USER_AGENT: &str = "pydantic-ai/1.107.0";

/// Anthropic API version header (the SDK default,
/// `anthropic/_client.py:395`).
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic base URL (the SDK default, `anthropic/_client.py:241`;
/// CE never configures one, `llm.py:103`).
pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";

/// Stainless retry contract (`openai/_constants.py:10-14`, identical
/// in `anthropic/_constants.py`): the canonical
/// [`pidash_services::assistant::retry`] policy (OVL-07 moved the
/// former private copy there; the turn streaming loop shares it).
pub use pidash_services::assistant::retry::MAX_RETRIES;

/// A provider lane: the [`ModelRef`][pidash_services::assistant::llm::ModelRef]
/// branch plus the creator's decrypted key.
#[derive(Debug, Clone)]
pub enum Lane {
    /// `AnthropicModel(model_name, provider=AnthropicProvider(api_key))`
    /// (`llm.py:99-103`): the default endpoint, key in `x-api-key`.
    Anthropic { model: String, api_key: String },
    /// `OpenAIChatModel(model_name, provider=OpenAIProvider(base_url,
    /// api_key))` (`llm.py:105-111`): chat completions under the
    /// configured base URL, key as a bearer token.
    OpenAICompatible {
        model: String,
        base_url: String,
        api_key: String,
    },
}

impl Lane {
    /// The lane's model name (the requested name, used for usage
    /// extraction and error text).
    pub fn model_name(&self) -> &str {
        match self {
            Lane::Anthropic { model, .. } | Lane::OpenAICompatible { model, .. } => model,
        }
    }

    /// `Model.system`: the provider tag used in error text
    /// (`openai.py:855`, `anthropic.py:502`).
    pub fn system(&self) -> &'static str {
        match self {
            Lane::Anthropic { .. } => "anthropic",
            Lane::OpenAICompatible { .. } => "openai",
        }
    }

    /// The provider name carried on responses (`provider.name`).
    pub fn provider_name(&self) -> &'static str {
        self.system()
    }

    /// The provider base URL carried on responses and fed to usage
    /// extraction.
    pub fn provider_url(&self) -> String {
        match self {
            Lane::Anthropic { .. } => format!("{ANTHROPIC_BASE_URL}/"),
            Lane::OpenAICompatible { base_url, .. } => {
                if base_url.ends_with('/') {
                    base_url.clone()
                } else {
                    format!("{base_url}/")
                }
            }
        }
    }

    /// The completions endpoint URL.
    pub fn endpoint_url(&self) -> String {
        match self {
            Lane::Anthropic { .. } => format!("{ANTHROPIC_BASE_URL}/v1/messages"),
            Lane::OpenAICompatible { base_url, .. } => {
                format!("{}/chat/completions", base_url.trim_end_matches('/'))
            }
        }
    }
}

/// Build the lane for a resolved model plus its decrypted key
/// (`build_model`, `llm.py:93-111`).
pub fn lane_for(model: &pidash_services::assistant::llm::ModelRef, api_key: &str) -> Lane {
    use pidash_services::assistant::llm::ModelRef;
    match model {
        ModelRef::Anthropic { model } => Lane::Anthropic {
            model: model.clone(),
            api_key: api_key.to_owned(),
        },
        ModelRef::OpenAICompatible { model, base_url } => Lane::OpenAICompatible {
            model: model.clone(),
            base_url: base_url.clone(),
            api_key: api_key.to_owned(),
        },
    }
}

// ---------------------------------------------------------------------------
// Wire tools: the per-lane schema transformers
// ---------------------------------------------------------------------------

/// One tool as sent on the wire: the transformed schema plus the
/// inferred `strict` flag (`_customize_tool_def`,
/// `models/__init__.py:1279-1291`).
#[derive(Debug, Clone, PartialEq)]
pub struct WireTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    /// `None` means the key is omitted from the wire tool.
    pub strict: Option<bool>,
}

/// Transform one raw tool schema for the OpenAI lane and infer its
/// `strict` flag (`OpenAIJsonSchemaTransformer`,
/// `profiles/openai.py:301-419`, `strict=None`).
pub fn transform_openai_schema(schema: &Value) -> (Value, bool) {
    let mut compatible = true;
    // `$defs` are walked separately and re-attached (`walk`, with
    // `prefer_inlined_defs=False`).
    let mut root = schema.clone();
    let defs = root
        .as_object_mut()
        .and_then(|map| map.remove("$defs"))
        .unwrap_or(Value::Null);
    let mut out = transform_openai_node(&root, &mut compatible);
    if let Value::Object(defs_map) = &defs {
        if !defs_map.is_empty() {
            let mut handled = Map::with_capacity(defs_map.len());
            for (key, value) in defs_map {
                handled.insert(key.clone(), transform_openai_node(value, &mut compatible));
            }
            if let Value::Object(out_map) = &mut out {
                out_map.insert("$defs".to_owned(), Value::Object(handled));
            }
        }
    }
    (out, compatible)
}

fn transform_openai_node(node: &Value, compatible: &mut bool) -> Value {
    let Value::Object(map) = node else {
        return node.clone();
    };
    // The base `_handle` walk: recurse into the node's children by
    // shape, then apply the leaf transform.
    let node_type = map.get("type").and_then(Value::as_str);
    let mut out = map.clone();
    if node_type == Some("object") {
        if let Some(Value::Object(props)) = map.get("properties") {
            let mut handled = Map::with_capacity(props.len());
            for (key, value) in props {
                handled.insert(key.clone(), transform_openai_node(value, compatible));
            }
            out.insert("properties".to_owned(), Value::Object(handled));
        }
        if let Some(additional) = map.get("additionalProperties") {
            if !additional.is_boolean() {
                out.insert(
                    "additionalProperties".to_owned(),
                    transform_openai_node(additional, compatible),
                );
            }
        }
        if let Some(Value::Object(patterns)) = map.get("patternProperties") {
            let mut handled = Map::with_capacity(patterns.len());
            for (key, value) in patterns {
                handled.insert(key.clone(), transform_openai_node(value, compatible));
            }
            out.insert("patternProperties".to_owned(), Value::Object(handled));
        }
    } else if node_type == Some("array") {
        if let Some(items) = map.get("items") {
            out.insert("items".to_owned(), transform_openai_node(items, compatible));
        }
        if let Some(Value::Array(prefix)) = map.get("prefixItems") {
            out.insert(
                "prefixItems".to_owned(),
                Value::Array(
                    prefix
                        .iter()
                        .map(|item| transform_openai_node(item, compatible))
                        .collect(),
                ),
            );
        }
    } else if node_type.is_none() {
        // `_handle_union` applies sequentially: `anyOf` first, then
        // `oneOf` over the result.
        for union in ["anyOf", "oneOf"] {
            let Some(Value::Array(members)) = out.remove(union) else {
                continue;
            };
            let handled: Vec<Value> = members
                .iter()
                .map(|member| transform_openai_node(member, compatible))
                .collect();
            if handled.len() == 1 {
                if let Value::Object(single) = &handled[0] {
                    // The single member wins; the node's own keys
                    // fill the gaps (`handled[0] | schema`). The
                    // loop continues so a remaining `oneOf` still
                    // processes, exactly like the chained
                    // `_handle_union` calls.
                    let mut merged = single.clone();
                    for (key, value) in &out {
                        merged.entry(key.clone()).or_insert_with(|| value.clone());
                    }
                    out = merged;
                    continue;
                }
            }
            out.insert(union.to_owned(), Value::Array(handled));
        }
    }
    transform_openai_leaf(&Value::Object(out), compatible)
}

/// The OpenAI leaf `transform` (`profiles/openai.py:333-419`).
fn transform_openai_leaf(node: &Value, compatible: &mut bool) -> Value {
    let Value::Object(map) = node else {
        return node.clone();
    };
    let mut out = map.clone();
    out.remove("title");
    out.remove("$schema");
    out.remove("discriminator");
    // `default` is kept but marks the schema incompatible.
    if out.contains_key("default") {
        *compatible = false;
    }
    // `$ref` siblings move into `anyOf` (strict-incompatible shape).
    if out.contains_key("$ref") && out.len() > 1 {
        if let Some(reference) = out.remove("$ref") {
            out.insert(
                "anyOf".to_owned(),
                Value::Array(vec![Value::Object(Map::from_iter([(
                    "$ref".to_owned(),
                    reference,
                )]))]),
            );
        }
    }
    // Strict-incompatible keys (`_STRICT_INCOMPATIBLE_KEYS`).
    for key in [
        "minLength",
        "maxLength",
        "patternProperties",
        "unevaluatedProperties",
        "propertyNames",
        "minProperties",
        "maxProperties",
        "unevaluatedItems",
        "contains",
        "minContains",
        "maxContains",
        "uniqueItems",
    ] {
        if out.contains_key(key) {
            *compatible = false;
        }
    }
    if let Some(format) = out.get("format").and_then(Value::as_str) {
        if ![
            "date-time",
            "time",
            "date",
            "duration",
            "email",
            "hostname",
            "ipv4",
            "ipv6",
            "uuid",
        ]
        .contains(&format)
        {
            *compatible = false;
        }
    }
    if let Some(pattern) = out.get("pattern").and_then(Value::as_str) {
        if regex_contains_lookaround(pattern) {
            *compatible = false;
        }
    }
    // `oneOf` is unsupported in strict mode.
    if out.contains_key("oneOf") {
        *compatible = false;
    }
    if out.get("type").and_then(Value::as_str) == Some("object") {
        // OpenAI drops objects without `properties`.
        if !out.contains_key("properties") {
            out.insert("properties".to_owned(), Value::Object(Map::new()));
        }
        match out.get("additionalProperties") {
            None => {
                out.insert("additionalProperties".to_owned(), Value::Bool(false));
            }
            Some(Value::Bool(false)) => {}
            _ => {
                *compatible = false;
            }
        }
        match (out.get("properties"), out.get("required")) {
            (Some(Value::Object(props)), Some(Value::Array(required))) => {
                let required: Vec<&str> = required.iter().filter_map(Value::as_str).collect();
                for key in props.keys() {
                    if !required.contains(&key.as_str()) {
                        *compatible = false;
                        break;
                    }
                }
            }
            _ => {
                *compatible = false;
            }
        }
    }
    Value::Object(out)
}

/// Unescaped `(?=` / `(?!` / `(?<=` / `(?<!` scan
/// (`_regex_contains_lookaround`, `profiles/openai.py:286-297`).
fn regex_contains_lookaround(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();
    let mut escaped = false;
    let mut index = 0;
    while index < bytes.len() {
        let char = bytes[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if char == b'\\' {
            escaped = true;
            index += 1;
            continue;
        }
        let rest = &pattern[index..];
        if rest.starts_with("(?=")
            || rest.starts_with("(?!")
            || rest.starts_with("(?<=")
            || rest.starts_with("(?<!")
        {
            return true;
        }
        index += 1;
    }
    false
}

/// Transform one raw tool schema for the Anthropic lane
/// (`AnthropicJsonSchemaTransformer`, `providers/anthropic.py:122-182`):
/// only `title`/`$schema` are stripped, and `strict` always infers
/// false unless explicitly set (never here).
pub fn transform_anthropic_schema(schema: &Value) -> Value {
    transform_anthropic_node(schema)
}

/// Transform one tool schema for the lane and infer `strict`
/// (`_customize_tool_def` through the model profile; the turn path never
/// sets `strict`). The assistant's leg conversion calls this so the
/// caller holds no engine logic (agent naming convention P2).
pub fn wire_tool_for(lane: &Lane, name: &str, description: &str, parameters: &Value) -> WireTool {
    if matches!(lane, Lane::Anthropic { .. }) {
        WireTool {
            name: name.to_owned(),
            description: description.to_owned(),
            parameters: transform_anthropic_schema(parameters),
            strict: None,
        }
    } else {
        let (parameters, _compatible) = transform_openai_schema(parameters);
        WireTool {
            name: name.to_owned(),
            description: description.to_owned(),
            parameters,
            strict: None,
        }
    }
}

/// Convert a batch of `(name, description, parameters)` schemas for
/// the lane, in order (agent naming convention P2, section 4.3).
pub fn wire_tools_for(lane: &Lane, schemas: &[(String, String, Value)]) -> Vec<WireTool> {
    schemas
        .iter()
        .map(|(name, description, parameters)| wire_tool_for(lane, name, description, parameters))
        .collect()
}

fn transform_anthropic_node(node: &Value) -> Value {
    let Value::Object(map) = node else {
        return node.clone();
    };
    let mut out = Map::with_capacity(map.len());
    for (key, value) in map {
        if key == "title" || key == "$schema" {
            continue;
        }
        if (key == "properties" || key == "patternProperties" || key == "$defs")
            && matches!(value, Value::Object(_))
        {
            let Value::Object(props) = value else {
                continue;
            };
            let mut handled = Map::with_capacity(props.len());
            for (prop, sub) in props {
                handled.insert(prop.clone(), transform_anthropic_node(sub));
            }
            out.insert(key.clone(), Value::Object(handled));
        } else if key == "additionalProperties" && !value.is_boolean() {
            out.insert(key.clone(), transform_anthropic_node(value));
        } else if key == "items" || key == "prefixItems" {
            if let Value::Array(items) = value {
                out.insert(
                    key.clone(),
                    Value::Array(items.iter().map(transform_anthropic_node).collect()),
                );
            } else {
                out.insert(key.clone(), transform_anthropic_node(value));
            }
        } else if (key == "anyOf" || key == "oneOf") && matches!(value, Value::Array(_)) {
            let Value::Array(members) = value else {
                continue;
            };
            out.insert(
                key.clone(),
                Value::Array(members.iter().map(transform_anthropic_node).collect()),
            );
        } else {
            out.insert(key.clone(), value.clone());
        }
    }
    Value::Object(out)
}

// ---------------------------------------------------------------------------
// Conversation history: the `ModelMessage` subset the loop needs
// ---------------------------------------------------------------------------

/// `FinishReason` (`messages.py`): the mapped finish reason carried on
/// a model response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finish {
    Stop,
    Length,
    ToolCall,
    ContentFilter,
}

/// The `INVALID_JSON` wrap key for malformed tool-call arguments
/// (`messages.py:36-40`, the Anthropic-docs recommendation): calls
/// whose arguments are not a JSON object still round-trip through
/// the next request under this key.
pub const INVALID_JSON_KEY: &str = "INVALID_JSON";

/// Tool-call arguments (`ToolCallPart.args`): the OpenAI lane
/// carries the raw arguments string; the Anthropic lane a parsed
/// input object.
#[derive(Debug, Clone)]
pub enum ToolArgs {
    Dict(Map<String, Value>),
    Raw(String),
}

/// One tool call in a model response (`ToolCallPart`).
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub args: ToolArgs,
    pub call_id: String,
}

impl ToolCall {
    /// `args_as_dict()` (`messages.py:1838-1847`): falsy args read
    /// `{}`, dicts as-is; strings parsed, falling back to the
    /// `INVALID_JSON` wrap when the payload is not a JSON object.
    pub fn args_as_dict(&self) -> Map<String, Value> {
        match &self.args {
            ToolArgs::Dict(map) => map.clone(),
            ToolArgs::Raw(raw) if raw.is_empty() => Map::new(),
            ToolArgs::Raw(raw) => match serde_json::from_str::<Value>(raw) {
                Ok(Value::Object(map)) => map,
                _ => Map::from_iter([(INVALID_JSON_KEY.to_owned(), Value::String(raw.clone()))]),
            },
        }
    }

    /// `args_as_json_str()` (`messages.py:1851-1860`): falsy args
    /// replay as `'{}'`; raw strings replay verbatim (the captured
    /// turn-2 `arguments` keep the provider's own spacing); dicts
    /// serialize compact (`pydantic_core.to_json`).
    pub fn args_as_json_str(&self) -> String {
        match &self.args {
            ToolArgs::Raw(raw) if raw.is_empty() => "{}".to_owned(),
            ToolArgs::Raw(raw) => raw.clone(),
            ToolArgs::Dict(map) => {
                serde_json::to_string(&Value::Object(map.clone())).unwrap_or("{}".to_owned())
            }
        }
    }
}

/// A provider failure, mirroring the SDK exception the lane would
/// have raised (`_map_api_errors`, `openai.py:184-191` and the
/// identical `anthropic.py:245`).
#[derive(Debug, Clone)]
pub enum ProviderError {
    /// `APIStatusError` → `ModelHTTPError(status_code, model_name,
    /// body)`: an HTTP error status after retries. `body_text` is
    /// the stripped text; `body_json` the parsed body when the
    /// payload is JSON (the SDK's `e.body` reads
    /// parsed-JSON-or-text).
    Http {
        status: u16,
        body_text: String,
        body_json: Option<Value>,
    },
    /// `APITimeoutError` → `ModelAPIError`: the per-request timeout
    /// fired on every attempt.
    Timeout,
    /// `APIConnectionError` → `ModelAPIError`: connection failed on
    /// every attempt.
    Connection,
    /// The endpoint answered 200 with an unusable payload
    /// (`_process_response` shape errors).
    InvalidResponse(String),
}

impl ProviderError {
    /// The `str(exc)` text the run classifier sees
    /// (`ModelHTTPError` / `ModelAPIError` messages,
    /// `exceptions.py:236-264`; SDK messages
    /// `openai/_exceptions.py:94-101`,
    /// `anthropic/_exceptions.py:96-103`).
    pub fn message(&self, lane: &Lane) -> String {
        match self {
            ProviderError::Http {
                status,
                body_text,
                body_json,
            } => {
                // `f'status_code: {s}, model_name: {m}, body: {body}'`
                // with the body Python-`repr`'d (a parsed dict) or raw
                // (text). On the OpenAI lane the SDK unwraps the
                // envelope first (`data = body.get("error", body) if
                // is_mapping(body) else body`, `_make_status_error`
                // in `openai/_client.py`); the Anthropic SDK has no
                // such unwrap, so that lane keeps the full body.
                let body = match body_json {
                    Some(json) => {
                        let shown = match (lane, json) {
                            (Lane::OpenAICompatible { .. }, Value::Object(map)) => {
                                map.get("error").unwrap_or(json)
                            }
                            _ => json,
                        };
                        py_repr(shown)
                    }
                    None => body_text.clone(),
                };
                format!(
                    "status_code: {status}, model_name: {}, body: {body}",
                    lane.model_name(),
                )
            }
            ProviderError::Timeout => match lane {
                Lane::Anthropic { .. } => "Request timed out or interrupted. This could be due to a network timeout, dropped connection, or request cancellation. See https://docs.anthropic.com/en/api/errors#long-requests for more details.".to_owned(),
                Lane::OpenAICompatible { .. } => "Request timed out.".to_owned(),
            },
            ProviderError::Connection => "Connection error.".to_owned(),
            ProviderError::InvalidResponse(text) => text.clone(),
        }
    }
}

/// Python `repr` for JSON values, as used in the `ModelHTTPError`
/// body slot: single-quoted strings with Python escaping,
/// `True`/`False`/`None`, compact containers.
pub fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => py_repr_str(text),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, val)| format!("{}: {}", py_repr_str(key), py_repr(val)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python single-quoted `repr` for one string: single quotes unless
/// the text contains one (but no double quote), with `\\`, `\n`,
/// `\r`, `\t` escaped and every other non-printable char
/// `\x..`-/`\u..`-escaped by width.
fn py_repr_str(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    out.push(quote);
    for char in text.chars() {
        match char {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if !py_printable(c) => {
                let code = c as u32;
                if code < 0x100 {
                    out.push_str(&format!("\\x{code:02x}"));
                } else if code < 0x1_0000 {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// `Py_UNICODE_ISPRINTABLE` (`str.isprintable`, probed across the BMP
/// against CPython 3.12): general categories Other/Separator except
/// SPACE. Unicode tables come from the locked `regex`, whose version
/// may be newer than 3.12's 15.0 — only chars assigned since (exotic
/// scripts in error bodies) can differ.
fn py_printable(candidate: char) -> bool {
    if candidate == ' ' {
        return true;
    }
    static NONPRINTABLE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pattern =
        NONPRINTABLE.get_or_init(|| regex::Regex::new(r"[\p{C}\p{Z}]").expect("class regex"));
    let mut encoded = [0u8; 4];
    !pattern.is_match(candidate.encode_utf8(&mut encoded))
}

/// `pyd_ai_{uuid4hex}` (`generate_tool_call_id`, `_utils.py:431-436`).
pub fn generate_tool_call_id() -> String {
    format!("pyd_ai_{}", uuid::Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_strict_inference_matches_catalog_capture() {
        // `wiretools.json`: strict holds exactly for the six
        // default-free, fully-required tools (plus the synthetic
        // cases above); every other tool infers false.
        //
        // P2: reads the test-only `provider-catalog` fixture (the
        // 13 catalog schemas + the output schema, extracted from
        // the retired fx-disp-08) so that fixture could be deleted
        // in P3 without losing this pin.
        const CATALOG: &str =
            include_str!("../../../../fixtures/cloud_agent/provider-catalog.golden.json");
        let catalog: Value = serde_json::from_str(CATALOG).expect("catalog parses");
        let mut strict_tools: Vec<&str> = Vec::new();
        for name in [
            "pidash_add_current_issue_comment",
            "pidash_create_project_issue",
            "pidash_get_current_issue",
            "pidash_get_project_issue",
            "pidash_list_current_issue_comments",
            "pidash_list_issue_relations",
            "pidash_list_linked_code_reviews",
            "pidash_list_project_states",
            "pidash_relate_issues",
            "pidash_search_project_issues",
            "pidash_transition_current_issue",
            "pidash_unrelate_issues",
            "pidash_update_current_issue_workpad",
        ] {
            let schema = catalog["function_schemas"][name]
                .get("parameters")
                .unwrap_or_else(|| panic!("catalog has {name}"));
            let (_, compatible) = transform_openai_schema(schema);
            if compatible {
                strict_tools.push(name);
            }
        }
        assert_eq!(
            strict_tools,
            [
                "pidash_add_current_issue_comment",
                "pidash_get_project_issue",
                "pidash_relate_issues",
                "pidash_transition_current_issue",
                "pidash_unrelate_issues",
                "pidash_update_current_issue_workpad",
            ]
        );
        let output_parameters = catalog["output_schema"]
            .get("parameters")
            .expect("catalog has output parameters");
        let (_, output_compatible) = transform_openai_schema(output_parameters);
        assert!(!output_compatible);
    }

    #[test]
    fn empty_raw_args_read_empty_object() {
        // `if not self.args: return {}` (`messages.py:1838-1839`).
        let call = ToolCall {
            name: "add".to_owned(),
            args: ToolArgs::Raw(String::new()),
            call_id: "c1".to_owned(),
        };
        assert!(call.args_as_dict().is_empty());
    }

    #[test]
    fn py_repr_escapes_nonprintables() {
        // `Py_UNICODE_ISPRINTABLE` (probed against CPython 3.12):
        // C1 controls `\x..`-escape, astral private-use
        // `\U..`-escapes, separators escape — printable text stays
        // raw, including astral emoji.
        for (text, expected) in [
            ("a\x7fb", "'a\\x7fb'"),
            ("a\u{2028}b", "'a\\u2028b'"),
            ("a\u{ad}b", "'a\\xadb'"),
            ("a\u{a0}b", "'a\\xa0b'"),
            ("a😀b", "'a😀b'"),
            ("a\u{e0001}b", "'a\\U000e0001b'"),
            ("aÿb", "'aÿb'"),
            ("café 中文", "'café 中文'"),
            ("a'b\"c", "'a\\'b\"c'"),
            ("don't", "\"don't\""),
            // C1 controls, written as escapes (invisible in source).
            ("a\u{80}b", "'a\\x80b'"),
            ("a\u{85}b", "'a\\x85b'"),
            ("a\u{9f}b", "'a\\x9fb'"),
        ] {
            assert_eq!(
                py_repr(&Value::String(text.to_owned())),
                expected,
                "{text:?}"
            );
        }
    }

    #[test]
    fn http_error_message_matches_sdk_shape() {
        let lane = Lane::OpenAICompatible {
            model: "gpt-4o-mini".to_owned(),
            base_url: "https://t/v1".to_owned(),
            api_key: "k".to_owned(),
        };
        let wrapped: Value = serde_json::from_str(
            r#"{"error":{"message":"bad key","type":"invalid_request_error","code":null}}"#,
        )
        .unwrap();
        let error = ProviderError::Http {
            status: 400,
            body_text: String::new(),
            body_json: Some(wrapped.clone()),
        };
        // The OpenAI lane unwraps `body["error"]` (`_make_status_error`).
        assert_eq!(
            error.message(&lane),
            "status_code: 400, model_name: gpt-4o-mini, body: {'message': 'bad key', 'type': 'invalid_request_error', 'code': None}"
        );
        // The Anthropic lane keeps the full body (no SDK unwrap).
        let anthropic = Lane::Anthropic {
            model: "claude-test".to_owned(),
            api_key: "k".to_owned(),
        };
        let error = ProviderError::Http {
            status: 400,
            body_text: String::new(),
            body_json: Some(wrapped),
        };
        assert_eq!(
            error.message(&anthropic),
            "status_code: 400, model_name: claude-test, body: {'error': {'message': 'bad key', 'type': 'invalid_request_error', 'code': None}}"
        );
        // Objects without an `error` key and non-object bodies repr whole.
        let error = ProviderError::Http {
            status: 400,
            body_text: String::new(),
            body_json: Some(serde_json::from_str(r#"{"message":"nope"}"#).unwrap()),
        };
        assert_eq!(
            error.message(&lane),
            "status_code: 400, model_name: gpt-4o-mini, body: {'message': 'nope'}"
        );
        let error = ProviderError::Http {
            status: 400,
            body_text: String::new(),
            body_json: Some(serde_json::from_str(r#"["oops"]"#).unwrap()),
        };
        assert_eq!(
            error.message(&lane),
            "status_code: 400, model_name: gpt-4o-mini, body: ['oops']"
        );
        assert_eq!(ProviderError::Timeout.message(&lane), "Request timed out.");
        assert_eq!(
            ProviderError::Connection.message(&lane),
            "Connection error."
        );
    }
}
