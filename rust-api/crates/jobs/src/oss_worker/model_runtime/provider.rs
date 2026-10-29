//! Provider HTTP lanes: Anthropic Messages + OpenAI-compatible Chat
//! Completions (`AnthropicModel` / `OpenAIChatModel`,
//! `pydantic_ai/models/{anthropic,openai}.py`, pinned 1.107.0).
//!
//! Covers exactly the configuration `cloud_agent/runtime.py` exercises:
//! non-streaming requests, `{max_tokens, timeout}` settings, thinking on
//! the OpenAI-compatible lane only (tags plus the `reasoning` /
//! `reasoning_content` fields), no cache controls, no native tools, no
//! compaction. Anything outside that surface (streaming, images, native
//! tools) is unreachable here because the run drive never sends it.
//!
//! Wire notes (each cites its probe or SDK line):
//!
//! * OpenAI: `POST {base_url}/chat/completions` with
//!   `{model, messages, tools, tool_choice: "required", stream: false,
//!   max_completion_tokens}` (`wirecap.json`, `loopprobe.json`); the
//!   per-request `timeout` is transport-level, never a body key
//!   (`edgeprobe.json` `settings_map`).
//! * Anthropic: `POST https://api.anthropic.com/v1/messages`
//!   (`anthropic/_client.py:241`, the provider default — CE never sets
//!   a base URL on this branch, `llm.py:103`) with
//!   `{model, system, messages, tools, tool_choice: {"type": "any"},
//!   max_tokens, stream: false}` (`anthropic.py:713-740`,
//!   `_tool_choice.py`, `_prepare_tools_and_tool_choice`); the env's
//!   anthropic 1.12.0 SDK cannot run this lane, so the wire shape is
//!   read from the pinned pydantic-ai source, not captured live.
//! * Both lanes send `User-Agent: pydantic-ai/1.107.0`
//!   (`get_user_agent`, set as an explicit extra header by both
//!   `_completions_create` and `_get_betas_and_extra_headers`). The
//!   SDKs' `X-Stainless-*` telemetry headers are deliberately not
//!   sent: they identify the Python HTTP stack, not the request.
//! * HTTP retries follow the shared stainless contract both SDKs
//!   implement (`openai/_base_client.py`, `anthropic/_base_client.py`):
//!   at most 2 retries on 408/409/429/5xx (plus the `x-should-retry`
//!   override), on timeouts, and on connection errors; backoff
//!   `min(0.5 * 2^n, 8) * (1 - 0.25 * rand)`; `retry-after-ms` or
//!   `retry-after` (seconds, float, or HTTP-date) honoured when within
//!   `(0, 60]`.
//! * Usage extraction delegates to [`crate::oss_worker::model_runtime::usage`]; the
//!   per-lane `details` shapes are read from each model's `_map_usage`.

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
use pidash_services::assistant::retry::{should_retry_status, sleep_for_retry};

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

/// A request-side part (`ModelRequestPart` subset: system prompts
/// never occur — instructions travel out-of-band).
#[derive(Debug, Clone)]
pub enum RequestPart {
    /// `UserPromptPart(content: str)`: the run prompt.
    UserPrompt(String),
    /// `ToolReturnPart`: a tool result.
    ToolReturn {
        // Kept for `ModelResponse` parity: neither wire format echoes
        // the tool name on results.
        #[allow(dead_code)]
        name: String,
        content: Value,
        call_id: String,
    },
    /// `RetryPromptPart`: validation/unknown-tool feedback.
    Retry {
        /// `None` for output retries (rendered as a user message on
        /// OpenAI, a text block on Anthropic).
        tool_name: Option<String>,
        content: RetryContent,
        call_id: String,
    },
}

/// Retry feedback content (`RetryPromptPart.content`).
#[derive(Debug, Clone)]
pub enum RetryContent {
    /// A `ModelRetry` message (unknown tool, or a tool's own retry
    /// request).
    Text(String),
    /// Pydantic error details (tool-arg or output validation).
    Errors(Vec<crate::oss_worker::model_runtime::validation::ValidationDetail>),
}

/// A response-side part (`ModelResponsePart` subset: no files or
/// native tools in this configuration; thinking occurs on the
/// OpenAI-compatible lane only).
#[derive(Debug, Clone)]
pub enum ResponsePart {
    Text(String),
    /// `ThinkingPart`: `id` is the source field (`reasoning`,
    /// `reasoning_content`) or `"content"` for tag-sourced thinking.
    /// The provider always matches this lane, and no custom thinking
    /// field is configured, so the id alone decides the replay shape
    /// (`_process_response`, `openai.py:1103-1110,1142-1176`).
    Thinking {
        id: String,
        content: String,
    },
    ToolCall(ToolCall),
}

/// One conversation turn (`ModelMessage`).
// The response arm is unavoidably larger (usage + metadata ride
// along); turns live in a `Vec` and move rarely.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum HistoryMessage {
    Request {
        parts: Vec<RequestPart>,
    },
    // The metadata rides along for `ModelResponse` parity; only
    // `parts`/`finish` are re-read (history replay, stub detection).
    #[allow(dead_code)]
    Response {
        parts: Vec<ResponsePart>,
        usage: crate::oss_worker::model_runtime::usage::RequestUsage,
        model_name: String,
        finish: Option<Finish>,
        provider_details: Option<Map<String, Value>>,
        response_id: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Request building
// ---------------------------------------------------------------------------

/// Render one tool-return value for the model (`model_response_str`,
/// `messages.py:1243-1252`): strings verbatim, anything else as
/// compact JSON (`tool_return_ta.dump_json`, separators `,`/`:` with
/// unescaped Unicode — exactly `serde_json::to_string`).
pub fn tool_result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or("null".to_owned()),
    }
}

/// Render one tool-return value for an OpenAI tool message
/// (`model_response_str`, `messages.py:1243-1250`): empty data — an
/// empty list or null — renders as `""` (`_unwrap_data`,
/// `messages.py:1179-1187`, returns None for it), anything else
/// through `tool_result_text`.
pub fn openai_tool_result_text(content: &Value) -> String {
    match content {
        Value::Array(items) if items.is_empty() => String::new(),
        Value::Null => String::new(),
        other => tool_result_text(other),
    }
}

/// Render retry feedback (`RetryPromptPart.model_response`,
/// `messages.py:1445-1467`): str content reads verbatim with a tool
/// name, `Validation feedback:\n`-prefixed without one; error lists
/// render as `{n} validation error(s):\n```json\n{errors}\n``` ` with
/// the `input`/`ctx` exclusions, always suffixed with
/// `\n\nFix the errors and try again.` Floats render via serde/ryu,
/// which diverges from CPython for large exponents (`1e30` vs
/// `1e+30`).
pub fn retry_message(content: &RetryContent, tool_name: Option<&str>) -> String {
    let description = match content {
        RetryContent::Text(text) => {
            if tool_name.is_none() {
                format!("Validation feedback:\n{text}")
            } else {
                text.clone()
            }
        }
        RetryContent::Errors(details) => {
            let errors: Vec<Value> = details
                .iter()
                .map(|detail| render_retry_error(detail, tool_name.is_none()))
                .collect();
            let pretty =
                serde_json::to_string_pretty(&Value::Array(errors)).unwrap_or("[]".to_owned());
            // `json.dumps(..., ensure_ascii=True)`.
            let body = crate::oss_worker::model_runtime::validation::ensure_ascii(&pretty);
            let plural = details.len() != 1;
            format!(
                "{} validation error{}:\n```json\n{body}\n```",
                details.len(),
                if plural { "s" } else { "" },
            )
        }
    };
    format!("{description}\n\nFix the errors and try again.")
}

/// One retry error object (`model_response` exclusions): without a
/// tool name, top-level errors (loc of length ≤ 1) drop `ctx` and
/// `input`; deeper ones drop `ctx`. With a tool name every error
/// drops `ctx` only. `url` is already excluded at collection
/// (`error.errors(include_url=False, include_context=False)`).
fn render_retry_error(
    detail: &crate::oss_worker::model_runtime::validation::ValidationDetail,
    no_tool: bool,
) -> Value {
    use crate::oss_worker::model_runtime::validation::LocSeg;
    let mut error = Map::new();
    error.insert(
        "type".to_owned(),
        Value::String(detail.error_type.to_owned()),
    );
    error.insert(
        "loc".to_owned(),
        Value::Array(
            detail
                .loc
                .iter()
                .map(|seg| match seg {
                    LocSeg::Field(name) => Value::String(name.clone()),
                    LocSeg::Index(index) => Value::Number((*index).into()),
                })
                .collect(),
        ),
    );
    error.insert("msg".to_owned(), Value::String(detail.msg.clone()));
    let drop_input = no_tool && detail.loc.len() <= 1;
    if !drop_input {
        error.insert("input".to_owned(), detail.input.clone());
    }
    Value::Object(error)
}

/// Build the OpenAI chat-completions body (`_completions_create` /
/// `_map_messages`, `openai.py:953-1040,1445-1494`) for this
/// configuration: `{model, messages, tools, tool_choice,
/// stream: false, max_completion_tokens}`. Instructions prepend as
/// the system message; empty requests and responses emit nothing.
pub fn openai_request_body(
    model: &str,
    instructions: &str,
    history: &[HistoryMessage],
    tools: &[WireTool],
    max_tokens: i64,
) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    for message in history {
        match message {
            HistoryMessage::Request { parts } => {
                for part in parts {
                    match part {
                        RequestPart::UserPrompt(text) => {
                            messages.push(openai_user_message(text));
                        }
                        RequestPart::ToolReturn {
                            name: _,
                            content,
                            call_id,
                        } => {
                            messages.push(Value::Object(Map::from_iter([
                                ("role".to_owned(), Value::String("tool".to_owned())),
                                ("tool_call_id".to_owned(), Value::String(call_id.to_owned())),
                                (
                                    "content".to_owned(),
                                    Value::String(openai_tool_result_text(content)),
                                ),
                            ])));
                        }
                        RequestPart::Retry {
                            tool_name,
                            content,
                            call_id,
                        } => {
                            let text = retry_message(content, tool_name.as_deref());
                            if tool_name.is_none() {
                                messages.push(openai_user_message(&text));
                            } else {
                                messages.push(Value::Object(Map::from_iter([
                                    ("role".to_owned(), Value::String("tool".to_owned())),
                                    ("tool_call_id".to_owned(), Value::String(call_id.to_owned())),
                                    ("content".to_owned(), Value::String(text)),
                                ])));
                            }
                        }
                    }
                }
            }
            HistoryMessage::Response { parts, .. } => {
                if let Some(mapped) = openai_assistant_message(parts) {
                    messages.push(mapped);
                }
            }
        }
    }
    if !instructions.is_empty() {
        // `openai_system_prompt_role` (`profiles/openai.py:218`,
        // `models/openai.py:1464`): o1-mini rejects the `system`
        // role, so its instructions travel as `user`. Either way
        // they insert after the leading same-role messages
        // (`system_prompt_count`; always 0 for `system` here since
        // no `SystemPromptPart`s ever occur).
        let role = if model.starts_with("o1-mini") {
            "user"
        } else {
            "system"
        };
        let at = messages
            .iter()
            .take_while(|message| message.get("role").and_then(Value::as_str) == Some(role))
            .count();
        messages.insert(at, openai_role_message(role, instructions));
    }
    let wire_tools: Vec<Value> = tools.iter().map(openai_wire_tool).collect();
    Value::Object(Map::from_iter([
        ("model".to_owned(), Value::String(model.to_owned())),
        ("messages".to_owned(), Value::Array(messages)),
        ("stream".to_owned(), Value::Bool(false)),
        (
            "tool_choice".to_owned(),
            Value::String("required".to_owned()),
        ),
        ("tools".to_owned(), Value::Array(wire_tools)),
        (
            "max_completion_tokens".to_owned(),
            Value::Number(max_tokens.into()),
        ),
    ]))
}

fn openai_user_message(content: &str) -> Value {
    Value::Object(Map::from_iter([
        ("role".to_owned(), Value::String("user".to_owned())),
        ("content".to_owned(), Value::String(content.to_owned())),
    ]))
}

fn openai_role_message(role: &str, content: &str) -> Value {
    Value::Object(Map::from_iter([
        ("role".to_owned(), Value::String(role.to_owned())),
        ("content".to_owned(), Value::String(content.to_owned())),
    ]))
}

/// The assistant replay (`_MapModelResponseContext`,
/// `openai.py:1293-1400`): texts joined with `\n\n`, thinking replayed
/// per the `auto` mode (field-sourced as `message[field]`,
/// tag-sourced as a `<think>` block inside the texts), `content: null`
/// without texts, tool calls in order; `None` when there is nothing
/// to send.
fn openai_assistant_message(parts: &[ResponsePart]) -> Option<Value> {
    let mut texts: Vec<String> = Vec::new();
    let mut thinkings: Vec<(&str, Vec<&str>)> = Vec::new();
    let mut calls: Vec<Value> = Vec::new();
    for part in parts {
        match part {
            ResponsePart::Text(text) => texts.push(text.clone()),
            // `_map_response_thinking_part`, `auto` mode
            // (`openai.py:1363-1394`): `id == "content"` means
            // tag-sourced, so tags mode; a field id replays as its
            // own field (the provider always matches this lane, and
            // no custom field is configured).
            ResponsePart::Thinking { id, content } => {
                if id == "content" {
                    let (start_tag, end_tag) = THINKING_TAGS;
                    texts.push([start_tag, content.as_str(), end_tag].join("\n"));
                } else if let Some((_, contents)) =
                    thinkings.iter_mut().find(|(field, _)| *field == id)
                {
                    contents.push(content.as_str());
                } else {
                    thinkings.push((id.as_str(), vec![content.as_str()]));
                }
            }
            ResponsePart::ToolCall(call) => {
                calls.push(Value::Object(Map::from_iter([
                    ("id".to_owned(), Value::String(call.call_id.clone())),
                    ("type".to_owned(), Value::String("function".to_owned())),
                    (
                        "function".to_owned(),
                        Value::Object(Map::from_iter([
                            ("name".to_owned(), Value::String(call.name.clone())),
                            (
                                "arguments".to_owned(),
                                Value::String(call.args_as_json_str()),
                            ),
                        ])),
                    ),
                ])));
            }
        }
    }
    if texts.is_empty() && thinkings.is_empty() && calls.is_empty() {
        return None;
    }
    let mut message = Map::from_iter([("role".to_owned(), Value::String("assistant".to_owned()))]);
    for (field, contents) in &thinkings {
        message.insert((*field).to_owned(), Value::String(contents.join("\n\n")));
    }
    if texts.is_empty() {
        message.insert("content".to_owned(), Value::Null);
    } else {
        message.insert("content".to_owned(), Value::String(texts.join("\n\n")));
    }
    if !calls.is_empty() {
        message.insert("tool_calls".to_owned(), Value::Array(calls));
    }
    Some(Value::Object(message))
}

fn openai_wire_tool(tool: &WireTool) -> Value {
    let mut function = Map::from_iter([
        ("name".to_owned(), Value::String(tool.name.clone())),
        (
            "description".to_owned(),
            Value::String(tool.description.clone()),
        ),
        ("parameters".to_owned(), tool.parameters.clone()),
    ]);
    // `strict` only when inferred true and the profile supports it
    // (`_map_tool_definition`, `openai.py:1507-1523`); the default
    // profile supports it.
    if tool.strict == Some(true) {
        function.insert("strict".to_owned(), Value::Bool(true));
    }
    Value::Object(Map::from_iter([
        ("type".to_owned(), Value::String("function".to_owned())),
        ("function".to_owned(), Value::Object(function)),
    ]))
}

/// Build the Anthropic messages body (`_messages_create` /
/// `_map_message`, `anthropic.py:682-740,1202-1622`) for this
/// configuration. `system` carries the instructions as a single text
/// block (the instruction-parts path — no `SystemPromptPart`s ever
/// occur); requests map to user messages, responses to assistant
/// messages; empty ones emit nothing.
pub fn anthropic_request_body(
    model: &str,
    instructions: &str,
    history: &[HistoryMessage],
    tools: &[WireTool],
    max_tokens: i64,
) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    for message in history {
        match message {
            HistoryMessage::Request { parts } => {
                let mut blocks: Vec<Value> = Vec::new();
                for part in parts {
                    match part {
                        RequestPart::UserPrompt(text) => {
                            // Empty text is skipped (`_map_user_prompt`).
                            if !text.is_empty() {
                                blocks.push(anthropic_text_block(text));
                            }
                        }
                        RequestPart::ToolReturn {
                            content, call_id, ..
                        } => {
                            blocks.push(anthropic_tool_result(call_id, content, false));
                        }
                        RequestPart::Retry {
                            tool_name,
                            content,
                            call_id,
                        } => {
                            let text = retry_message(content, tool_name.as_deref());
                            if tool_name.is_none() {
                                blocks.push(anthropic_text_block(&text));
                            } else {
                                blocks.push(Value::Object(Map::from_iter([
                                    ("tool_use_id".to_owned(), Value::String(call_id.to_owned())),
                                    ("type".to_owned(), Value::String("tool_result".to_owned())),
                                    ("content".to_owned(), Value::String(text)),
                                    ("is_error".to_owned(), Value::Bool(true)),
                                ])));
                            }
                        }
                    }
                }
                if !blocks.is_empty() {
                    messages.push(Value::Object(Map::from_iter([
                        ("role".to_owned(), Value::String("user".to_owned())),
                        ("content".to_owned(), Value::Array(blocks)),
                    ])));
                }
            }
            HistoryMessage::Response { parts, .. } => {
                let mut blocks: Vec<Value> = Vec::new();
                for part in parts {
                    match part {
                        ResponsePart::Text(text) => {
                            if !text.is_empty() {
                                blocks.push(anthropic_text_block(text));
                            }
                        }
                        // Thinking never occurs on this lane: the
                        // Anthropic parse emits only text and tool
                        // calls (thinking blocks are outside this
                        // configuration), so there is nothing to
                        // replay.
                        ResponsePart::Thinking { .. } => {}
                        ResponsePart::ToolCall(call) => {
                            blocks.push(Value::Object(Map::from_iter([
                                ("id".to_owned(), Value::String(call.call_id.clone())),
                                ("type".to_owned(), Value::String("tool_use".to_owned())),
                                ("name".to_owned(), Value::String(call.name.clone())),
                                ("input".to_owned(), Value::Object(call.args_as_dict())),
                            ])));
                        }
                    }
                }
                if !blocks.is_empty() {
                    messages.push(Value::Object(Map::from_iter([
                        ("role".to_owned(), Value::String("assistant".to_owned())),
                        ("content".to_owned(), Value::Array(blocks)),
                    ])));
                }
            }
        }
    }
    let wire_tools: Vec<Value> = tools.iter().map(anthropic_wire_tool).collect();
    // `supports_forced_tool_choice` (`profiles/anthropic.py:214-216`,
    // `models/anthropic.py:1146-1152`): these models reject `any`
    // outright, so forcing degrades to `auto`.
    let forced = !["claude-fable-5", "claude-mythos-5", "claude-mythos-preview"]
        .iter()
        .any(|prefix| model.starts_with(prefix));
    let mut body = Map::from_iter([
        ("max_tokens".to_owned(), Value::Number(max_tokens.into())),
        ("model".to_owned(), Value::String(model.to_owned())),
        ("messages".to_owned(), Value::Array(messages)),
        ("stream".to_owned(), Value::Bool(false)),
        ("tools".to_owned(), Value::Array(wire_tools)),
        (
            "tool_choice".to_owned(),
            Value::Object(Map::from_iter([(
                "type".to_owned(),
                Value::String(if forced { "any" } else { "auto" }.to_owned()),
            )])),
        ),
    ]);
    if !instructions.is_empty() {
        body.insert(
            "system".to_owned(),
            Value::Array(vec![anthropic_text_block(instructions)]),
        );
    }
    Value::Object(body)
}

fn anthropic_text_block(text: &str) -> Value {
    Value::Object(Map::from_iter([
        ("type".to_owned(), Value::String("text".to_owned())),
        ("text".to_owned(), Value::String(text.to_owned())),
    ]))
}

/// A success tool result: one text block per content item
/// (`content_items(mode='str')` — strings verbatim, anything else as
/// compact JSON), or `""` when there is no content.
fn anthropic_tool_result(call_id: &str, content: &Value, _is_error: bool) -> Value {
    let items: Vec<Value> = match content {
        Value::Array(items) if !items.is_empty() => items
            .iter()
            .map(|item| anthropic_text_block(&tool_result_text(item)))
            .collect(),
        Value::Array(_) => Vec::new(),
        single => vec![anthropic_text_block(&tool_result_text(single))],
    };
    // `content or ''`: an empty item list sends the empty string.
    let content = if items.is_empty() {
        Value::String(String::new())
    } else {
        Value::Array(items)
    };
    Value::Object(Map::from_iter([
        ("tool_use_id".to_owned(), Value::String(call_id.to_owned())),
        ("type".to_owned(), Value::String("tool_result".to_owned())),
        ("content".to_owned(), content),
        ("is_error".to_owned(), Value::Bool(false)),
    ]))
}

fn anthropic_wire_tool(tool: &WireTool) -> Value {
    // `strict` only when `f.strict` (`_map_tool_definition`,
    // `anthropic.py:1933-1950`); the Anthropic transformer never
    // infers it, so the key stays off here.
    let mut wire = Map::from_iter([
        ("name".to_owned(), Value::String(tool.name.clone())),
        (
            "description".to_owned(),
            Value::String(tool.description.clone()),
        ),
        ("input_schema".to_owned(), tool.parameters.clone()),
    ]);
    if tool.strict == Some(true) {
        wire.insert("strict".to_owned(), Value::Bool(true));
    }
    Value::Object(wire)
}

// ---------------------------------------------------------------------------
// HTTP: the stainless send/retry loop, per-lane response parsing
// ---------------------------------------------------------------------------

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

/// Parse one response into history parts (`_process_response`,
/// `openai.py:1056-1140` / `anthropic.py:901-973`).
#[derive(Debug)]
pub struct ParsedResponse {
    pub parts: Vec<ResponsePart>,
    pub usage: crate::oss_worker::model_runtime::usage::RequestUsage,
    pub model_name: String,
    pub finish: Option<Finish>,
    pub provider_details: Option<Map<String, Value>>,
    pub response_id: Option<String>,
}

/// The default profile thinking tags (`profiles/__init__.py:125`);
/// `openai_model_profile` does not override them.
const THINKING_TAGS: (&str, &str) = ("<think>", "</think>");

/// `split_content_into_text_and_thinking` (`_thinking_part.py:6-31`):
/// tag-sourced thinking carries `id: "content"` (`openai.py:1108`).
/// Empty think spans still emit a part, an unclosed tag folds the
/// rest into text (tag lost), and empty text spans emit nothing.
fn split_content_into_text_and_thinking(content: &str) -> Vec<ResponsePart> {
    let (start_tag, end_tag) = THINKING_TAGS;
    let mut parts = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find(start_tag) {
        let (before, after_start) = rest.split_at(start);
        if !before.is_empty() {
            parts.push(ResponsePart::Text(before.to_owned()));
        }
        let after_tag = &after_start[start_tag.len()..];
        match after_tag.find(end_tag) {
            Some(end) => {
                let (think, after_end) = after_tag.split_at(end);
                parts.push(ResponsePart::Thinking {
                    id: "content".to_owned(),
                    content: think.to_owned(),
                });
                rest = &after_end[end_tag.len()..];
            }
            None => {
                parts.push(ResponsePart::Text(after_tag.to_owned()));
                rest = "";
            }
        }
    }
    if !rest.is_empty() {
        parts.push(ResponsePart::Text(rest.to_owned()));
    }
    parts
}

/// `_process_thinking` (`openai.py:1142-1176`) for this
/// configuration: no custom field, so `reasoning` then
/// `reasoning_content`; the first non-empty string wins and anything
/// else (missing, null, empty, non-string) falls through to the next
/// field, like the SDK's warn-and-continue.
fn openai_thinking_part(message: &Map<String, Value>) -> Option<ResponsePart> {
    for field in ["reasoning", "reasoning_content"] {
        match message.get(field) {
            Some(Value::String(reasoning)) if !reasoning.is_empty() => {
                return Some(ResponsePart::Thinking {
                    id: field.to_owned(),
                    content: reasoning.clone(),
                });
            }
            _ => continue,
        }
    }
    None
}

/// Parse an OpenAI chat-completions payload. `request_id` feeds the
/// Ollama-`None` workaround only through the value itself.
pub fn parse_openai_response(
    lane: &Lane,
    payload: &Value,
    extract: &crate::oss_worker::model_runtime::usage::ProviderTable,
) -> Result<ParsedResponse, ProviderError> {
    let invalid = |detail: &str| {
        ProviderError::InvalidResponse(format!(
            "Invalid response from {} chat completions endpoint{detail}",
            lane.system(),
        ))
    };
    let Value::Object(body) = payload else {
        return Err(invalid(", expected JSON data"));
    };
    let choices = body.get("choices").and_then(Value::as_array);
    let choice = choices
        .and_then(|list| list.first())
        .and_then(Value::as_object);
    let Some(choice) = choice else {
        return Err(invalid(", expected JSON data"));
    };
    let message = choice.get("message").and_then(Value::as_object);
    let Some(message) = message else {
        return Err(invalid(", expected JSON data"));
    };
    let model_name = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let response_id = body.get("id").and_then(Value::as_str).map(str::to_owned);
    // The Ollama `None` finish reason reads as `stop`.
    let raw_finish = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .unwrap_or("stop");
    let usage = openai_request_usage(lane, body, extract);
    // Refusals short-circuit with empty parts (`_process_response`).
    if let Some(refusal) = message.get("refusal").and_then(Value::as_str) {
        let mut details = Map::new();
        details.insert("refusal".to_owned(), Value::String(refusal.to_owned()));
        details.insert(
            "timestamp".to_owned(),
            Value::String(number_to_datetime(response_created(body))),
        );
        return Ok(ParsedResponse {
            parts: Vec::new(),
            usage,
            model_name,
            finish: Some(Finish::ContentFilter),
            provider_details: Some(details),
            response_id,
        });
    }
    let mut parts: Vec<ResponsePart> = Vec::new();
    // Field-sourced thinking lands first (`openai.py:1103-1104`).
    if let Some(thinking) = openai_thinking_part(message) {
        parts.push(thinking);
    }
    if let Some(content) = message.get("content").and_then(Value::as_str) {
        if !content.is_empty() {
            parts.extend(split_content_into_text_and_thinking(content));
        }
    }
    if let Some(Value::Array(calls)) = message.get("tool_calls") {
        for call in calls {
            let Some(call) = call.as_object() else {
                continue;
            };
            let function = call.get("function").and_then(Value::as_object);
            let (Some(name), Some(arguments)) = (
                function.and_then(|f| f.get("name")).and_then(Value::as_str),
                function
                    .and_then(|f| f.get("arguments"))
                    .and_then(Value::as_str),
            ) else {
                continue;
            };
            // Custom tool calls raise `RuntimeError('Custom tool
            // calls are not supported')`; only `function` calls
            // parse here, anything else is skipped the way the SDK
            // validation would have rejected it upstream.
            if call.get("type").and_then(Value::as_str) != Some("function") {
                return Err(ProviderError::InvalidResponse(
                    "Custom tool calls are not supported".to_owned(),
                ));
            }
            parts.push(ResponsePart::ToolCall(ToolCall {
                name: name.to_owned(),
                args: ToolArgs::Raw(arguments.to_owned()),
                // `t.tool_call_id or generate_tool_call_id()`
                // (`_utils.py:419`): missing AND empty ids regenerate.
                call_id: call
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(generate_tool_call_id),
            }));
        }
    }
    let mut details: Option<Map<String, Value>> = None;
    if !raw_finish.is_empty() {
        details.get_or_insert_with(Map::new).insert(
            "finish_reason".to_owned(),
            Value::String(raw_finish.to_owned()),
        );
    }
    details.get_or_insert_with(Map::new).insert(
        "timestamp".to_owned(),
        Value::String(number_to_datetime(response_created(body))),
    );
    // Unknown reasons map to `None` (the `.get` miss); the parsed
    // parts still stand.
    let finish = match raw_finish {
        "stop" => Some(Finish::Stop),
        "length" => Some(Finish::Length),
        "tool_calls" | "function_call" => Some(Finish::ToolCall),
        "content_filter" => Some(Finish::ContentFilter),
        _ => None,
    };
    Ok(ParsedResponse {
        parts,
        usage,
        model_name,
        finish,
        provider_details: details,
        response_id,
    })
}

/// `pyd_ai_{uuid4hex}` (`generate_tool_call_id`, `_utils.py:431-436`).
pub fn generate_tool_call_id() -> String {
    format!("pyd_ai_{}", uuid::Uuid::new_v4().simple())
}

/// The response `created` stamp (`openai.py:1068-1069`): falsy
/// values backfill with now, so the `timestamp` detail is always
/// present.
fn response_created(body: &Map<String, Value>) -> i64 {
    body.get("created")
        .and_then(Value::as_i64)
        .filter(|created| *created != 0)
        .unwrap_or_else(|| chrono::Utc::now().timestamp())
}

/// `number_to_datetime`: epoch seconds rendered UTC ISO-8601.
fn number_to_datetime(epoch: i64) -> String {
    chrono::DateTime::from_timestamp(epoch, 0)
        .map(|when| when.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

/// The OpenAI usage pull (`_map_usage`, `openai.py:4223-4261`):
/// a missing or non-object `usage` reads zeros, exactly like the
/// SDK's `response_usage is None` arm.
fn openai_request_usage(
    lane: &Lane,
    body: &Map<String, Value>,
    extract: &crate::oss_worker::model_runtime::usage::ProviderTable,
) -> crate::oss_worker::model_runtime::usage::RequestUsage {
    let usage = body.get("usage").and_then(Value::as_object);
    extract.extract_openai(&lane.provider_url(), lane.model_name(), usage)
}

/// Parse an Anthropic messages payload (`_process_response`,
/// `anthropic.py:901-973`).
pub fn parse_anthropic_response(
    lane: &Lane,
    payload: &Value,
    extract: &crate::oss_worker::model_runtime::usage::ProviderTable,
) -> Result<ParsedResponse, ProviderError> {
    let invalid = || {
        ProviderError::InvalidResponse(format!(
            "Invalid response from {} messages endpoint, expected JSON data",
            lane.system(),
        ))
    };
    let Value::Object(body) = payload else {
        return Err(invalid());
    };
    let Value::Array(blocks) = body.get("content").unwrap_or(&Value::Null) else {
        return Err(invalid());
    };
    let mut parts: Vec<ResponsePart> = Vec::new();
    for block in blocks {
        let Value::Object(block) = block else {
            return Err(invalid());
        };
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                parts.push(ResponsePart::Text(
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                ));
            }
            Some("tool_use") => {
                let Value::Object(input) = block.get("input").unwrap_or(&Value::Null) else {
                    return Err(invalid());
                };
                parts.push(ResponsePart::ToolCall(ToolCall {
                    name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    args: ToolArgs::Dict(input.clone()),
                    // `t.tool_call_id or generate_tool_call_id()`
                    // (`_utils.py:419`): missing AND empty ids
                    // regenerate.
                    call_id: block
                        .get("id")
                        .and_then(Value::as_str)
                        .filter(|id| !id.is_empty())
                        .map(str::to_owned)
                        .unwrap_or_else(generate_tool_call_id),
                }));
            }
            // The SDK asserts `BetaToolUseBlock` for anything else
            // (`assert isinstance(...)`, `anthropic.py:936`).
            other => {
                return Err(ProviderError::InvalidResponse(format!(
                    "unexpected item type {other:?}"
                )));
            }
        }
    }
    let raw_stop = body.get("stop_reason").and_then(Value::as_str);
    let mut details: Option<Map<String, Value>> = None;
    if let Some(stop) = raw_stop {
        details
            .get_or_insert_with(Map::new)
            .insert("finish_reason".to_owned(), Value::String(stop.to_owned()));
    }
    if let Value::Object(stop_details) = body.get("stop_details").unwrap_or(&Value::Null) {
        if let Some(explanation) = stop_details.get("explanation").and_then(Value::as_str) {
            details
                .get_or_insert_with(Map::new)
                .insert("refusal".to_owned(), Value::String(explanation.to_owned()));
        }
        if let Some(category) = stop_details.get("category").and_then(Value::as_str) {
            details.get_or_insert_with(Map::new).insert(
                "refusal_category".to_owned(),
                Value::String(category.to_owned()),
            );
        }
    }
    let finish = raw_stop.and_then(|stop| match stop {
        "compaction" | "end_turn" | "stop_sequence" | "pause_turn" => Some(Finish::Stop),
        "max_tokens" | "model_context_window_exceeded" => Some(Finish::Length),
        "tool_use" => Some(Finish::ToolCall),
        "refusal" => Some(Finish::ContentFilter),
        _ => None,
    });
    let usage = anthropic_request_usage(lane, body, extract);
    Ok(ParsedResponse {
        parts,
        usage,
        model_name: body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        finish,
        provider_details: details,
        response_id: body.get("id").and_then(Value::as_str).map(str::to_owned),
    })
}

/// The Anthropic usage pull (`_map_usage`, `anthropic.py:2141-2175`).
/// Compaction iteration totals never occur here: compaction only
/// happens when the request configures `context_management`, which
/// this configuration never sends.
fn anthropic_request_usage(
    lane: &Lane,
    body: &Map<String, Value>,
    extract: &crate::oss_worker::model_runtime::usage::ProviderTable,
) -> crate::oss_worker::model_runtime::usage::RequestUsage {
    let Value::Object(usage) = body.get("usage").unwrap_or(&Value::Null) else {
        return crate::oss_worker::model_runtime::usage::RequestUsage::zeros();
    };
    extract.extract_anthropic(&lane.provider_url(), usage)
}

// ---------------------------------------------------------------------------
// The send loop
// ---------------------------------------------------------------------------

/// One model turn's inputs.
pub struct Turn<'a> {
    pub lane: &'a Lane,
    pub instructions: &'a str,
    pub history: &'a [HistoryMessage],
    pub tools: &'a [WireTool],
    pub max_tokens: i64,
    /// Per-request timeout, seconds (`model_settings["timeout"]`).
    pub timeout_secs: u64,
    pub extract: &'a crate::oss_worker::model_runtime::usage::ProviderTable,
}

/// POST one turn and parse the response (`request`,
/// `openai.py:947-966` / `anthropic.py:511-525`): the stainless
/// retry loop around a single attempt, then the lane parser.
pub async fn complete(
    client: &reqwest::Client,
    turn: &Turn<'_>,
) -> Result<ParsedResponse, ProviderError> {
    let body = match turn.lane {
        Lane::Anthropic { model, .. } => anthropic_request_body(
            model,
            turn.instructions,
            turn.history,
            turn.tools,
            turn.max_tokens,
        ),
        Lane::OpenAICompatible { model, .. } => openai_request_body(
            model,
            turn.instructions,
            turn.history,
            turn.tools,
            turn.max_tokens,
        ),
    };
    let payload = post_with_retries(client, turn.lane, &body, turn.timeout_secs).await?;
    // The 200 payload parses exactly once: JSON-shape failures are
    // returned, never retried (SDK body parsing sits outside the
    // retry loop).
    let value: Value = serde_json::from_slice(&payload).map_err(|_| match turn.lane {
        Lane::Anthropic { .. } => ProviderError::InvalidResponse(format!(
            "Invalid response from {} messages endpoint, expected JSON data",
            turn.lane.system(),
        )),
        Lane::OpenAICompatible { .. } => ProviderError::InvalidResponse(format!(
            "Invalid response from {} chat completions endpoint, expected JSON data",
            turn.lane.system(),
        )),
    })?;
    match turn.lane {
        Lane::Anthropic { .. } => parse_anthropic_response(turn.lane, &value, turn.extract),
        Lane::OpenAICompatible { .. } => parse_openai_response(turn.lane, &value, turn.extract),
    }
}

/// The error-status body split (`_make_status_error_from_response`,
/// `openai/_base_client.py:407-427`, identical in the Anthropic
/// SDK): strip the text first, then parse-or-keep, so both the
/// JSON and the text slots carry the stripped form.
fn parse_error_body(bytes: &[u8]) -> (String, Option<Value>) {
    let text = String::from_utf8_lossy(bytes).trim().to_owned();
    let json = serde_json::from_str::<Value>(&text).ok();
    (text, json)
}

/// POST with the stainless retry loop (`openai/_base_client.py:990-1060`,
/// async mirror at 1610-1660; identical in the Anthropic SDK):
/// timeouts and connection errors retry while attempts remain, else
/// surface; error statuses retry per `_should_retry`, else surface
/// with the parsed-or-text body.
async fn post_with_retries(
    client: &reqwest::Client,
    lane: &Lane,
    body: &Value,
    timeout_secs: u64,
) -> Result<Vec<u8>, ProviderError> {
    let mut retries_taken: u32 = 0;
    loop {
        let mut request = client
            .post(lane.endpoint_url())
            .header("Content-Type", "application/json")
            .header("User-Agent", USER_AGENT)
            .timeout(std::time::Duration::from_secs(timeout_secs.max(1)));
        match lane {
            Lane::Anthropic { api_key, .. } => {
                request = request
                    .header("x-api-key", api_key.as_str())
                    .header("anthropic-version", ANTHROPIC_VERSION);
            }
            Lane::OpenAICompatible { api_key, .. } => {
                request = request.bearer_auth(api_key);
            }
        }
        let encoded = serde_json::to_vec(body).unwrap_or_default();
        match request.body(encoded).send().await {
            Err(error) if error.is_timeout() => {
                if retries_taken < MAX_RETRIES {
                    sleep_for_retry(retries_taken, None).await;
                    retries_taken += 1;
                    continue;
                }
                return Err(ProviderError::Timeout);
            }
            Err(_) => {
                // Any other send failure reads as a connection error
                // (the SDK's bare `except Exception` arm).
                if retries_taken < MAX_RETRIES {
                    sleep_for_retry(retries_taken, None).await;
                    retries_taken += 1;
                    continue;
                }
                return Err(ProviderError::Connection);
            }
            Ok(response) => {
                let status = response.status().as_u16();
                let headers = response.headers().clone();
                // The non-streaming SDK `send` reads the body inside the
                // request try block, so a body-read failure (read timeout
                // or mid-body reset) retries exactly like a send failure.
                let bytes = match response.bytes().await {
                    Ok(body) => body.to_vec(),
                    Err(error) if error.is_timeout() => {
                        if retries_taken < MAX_RETRIES {
                            sleep_for_retry(retries_taken, None).await;
                            retries_taken += 1;
                            continue;
                        }
                        return Err(ProviderError::Timeout);
                    }
                    Err(_) => {
                        if retries_taken < MAX_RETRIES {
                            sleep_for_retry(retries_taken, None).await;
                            retries_taken += 1;
                            continue;
                        }
                        return Err(ProviderError::Connection);
                    }
                };
                if status < 400 {
                    return Ok(bytes);
                }
                if retries_taken < MAX_RETRIES && should_retry_status(status, &headers) {
                    sleep_for_retry(retries_taken, Some(&headers)).await;
                    retries_taken += 1;
                    continue;
                }
                let (body_text, body_json) = parse_error_body(&bytes);
                return Err(ProviderError::Http {
                    status,
                    body_text,
                    body_json,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oss_worker::model_runtime::schemas::Golden;
    use crate::oss_worker::model_runtime::usage::ProviderTable;

    /// The captured turn-1 OpenAI request (`wirecap.json`,
    /// `Agent(OpenAIModel, output_type=Sum)` + `add` tool, no
    /// instructions, no model settings).
    const CAPTURED_REQ0: &str = r#"{"messages":[{"role":"user","content":"add 2 and 3"}],"model":"gpt-4o-mini","stream":false,"tool_choice":"required","tools":[{"type":"function","function":{"name":"add","description":"Add two numbers.","parameters":{"additionalProperties":false,"properties":{"a":{"type":"integer"},"b":{"type":"integer"}},"required":["a","b"],"type":"object"},"strict":true}},{"type":"function","function":{"name":"final_result","description":"The final response which ends this conversation","parameters":{"properties":{"sum":{"type":"integer"}},"required":["sum"],"type":"object","additionalProperties":false},"strict":true}}]}"#;

    /// The captured turn-2 request: assistant replay (verbatim
    /// arguments echo) plus the tool result.
    const CAPTURED_REQ1: &str = r#"{"messages":[{"role":"user","content":"add 2 and 3"},{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"add","arguments":"{\"a\": 2, \"b\": \"3\"}"}}]},{"role":"tool","tool_call_id":"call_1","content":"5"}],"model":"gpt-4o-mini","stream":false,"tool_choice":"required","tools":[{"type":"function","function":{"name":"add","description":"Add two numbers.","parameters":{"additionalProperties":false,"properties":{"a":{"type":"integer"},"b":{"type":"integer"}},"required":["a","b"],"type":"object"},"strict":true}},{"type":"function","function":{"name":"final_result","description":"The final response which ends this conversation","parameters":{"properties":{"sum":{"type":"integer"}},"required":["sum"],"type":"object","additionalProperties":false},"strict":true}}]}"#;

    fn wire_add_tools() -> Vec<WireTool> {
        let raw_add: Value = serde_json::from_str(
            r#"{"additionalProperties":false,"properties":{"a":{"type":"integer"},"b":{"type":"integer"}},"required":["a","b"],"type":"object"}"#,
        )
        .unwrap();
        let raw_final: Value = serde_json::from_str(
            r#"{"properties":{"sum":{"type":"integer"}},"required":["sum"],"type":"object"}"#,
        )
        .unwrap();
        let (add_params, add_strict) = transform_openai_schema(&raw_add);
        let (final_params, final_strict) = transform_openai_schema(&raw_final);
        vec![
            WireTool {
                name: "add".to_owned(),
                description: "Add two numbers.".to_owned(),
                parameters: add_params,
                strict: add_strict.then_some(true),
            },
            WireTool {
                name: "final_result".to_owned(),
                description: "The final response which ends this conversation".to_owned(),
                parameters: final_params,
                strict: final_strict.then_some(true),
            },
        ]
    }

    #[test]
    fn openai_turn_one_matches_capture() {
        let tools = wire_add_tools();
        let history = vec![HistoryMessage::Request {
            parts: vec![RequestPart::UserPrompt("add 2 and 3".to_owned())],
        }];
        // No model settings in the capture: `max_tokens` never
        // defaults (`model_settings.get('max_tokens', OMIT)`), so
        // the key is absent. The body builder always takes the
        // setting; compare without it by removing the test-only key.
        let mut body = openai_request_body("gpt-4o-mini", "", &history, &tools, 0);
        body.as_object_mut()
            .unwrap()
            .remove("max_completion_tokens");
        let expected: Value = serde_json::from_str(CAPTURED_REQ0).unwrap();
        assert_eq!(body, expected);
    }

    #[test]
    fn openai_turn_two_matches_capture() {
        let tools = wire_add_tools();
        let history = vec![
            HistoryMessage::Request {
                parts: vec![RequestPart::UserPrompt("add 2 and 3".to_owned())],
            },
            HistoryMessage::Response {
                parts: vec![ResponsePart::ToolCall(ToolCall {
                    name: "add".to_owned(),
                    args: ToolArgs::Raw("{\"a\": 2, \"b\": \"3\"}".to_owned()),
                    call_id: "call_1".to_owned(),
                })],
                usage: crate::oss_worker::model_runtime::usage::RequestUsage::zeros(),
                model_name: String::new(),
                finish: Some(Finish::ToolCall),
                provider_details: None,
                response_id: None,
            },
            HistoryMessage::Request {
                parts: vec![RequestPart::ToolReturn {
                    name: "add".to_owned(),
                    content: Value::Number(5.into()),
                    call_id: "call_1".to_owned(),
                }],
            },
        ];
        let mut body = openai_request_body("gpt-4o-mini", "", &history, &tools, 0);
        body.as_object_mut()
            .unwrap()
            .remove("max_completion_tokens");
        let expected: Value = serde_json::from_str(CAPTURED_REQ1).unwrap();
        assert_eq!(body, expected);
    }

    #[test]
    fn o1_mini_instructions_use_user_role() {
        // `openai_system_prompt_role` (`profiles/openai.py:218`):
        // o1-mini instructions travel as `user`, after the leading
        // user messages; anything else stays `system` at index 0.
        let history = vec![HistoryMessage::Request {
            parts: vec![RequestPart::UserPrompt("go".to_owned())],
        }];
        let body = openai_request_body("o1-mini-2024-09-12", "Behave.", &history, &[], 0);
        assert_eq!(
            body["messages"],
            serde_json::json!([
                {"role": "user", "content": "go"},
                {"role": "user", "content": "Behave."},
            ])
        );
        let body = openai_request_body("gpt-4o-mini", "Behave.", &history, &[], 0);
        assert_eq!(
            body["messages"],
            serde_json::json!([
                {"role": "system", "content": "Behave."},
                {"role": "user", "content": "go"},
            ])
        );
    }

    #[test]
    fn openai_strict_inference_matches_catalog_capture() {
        // `wiretools.json`: strict holds exactly for the six
        // default-free, fully-required tools (plus the synthetic
        // cases above); every other tool infers false.
        let golden = Golden::load().expect("golden parses");
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
            let schema = golden.function_schema(name).unwrap_or_else(|| {
                panic!("golden has {name}");
            });
            let (_, compatible) = transform_openai_schema(&schema.parameters);
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
        let (_, output_compatible) = transform_openai_schema(&golden.output_schema().parameters);
        assert!(!output_compatible);
    }

    #[test]
    fn anthropic_body_shape() {
        let tools = vec![WireTool {
            name: "add".to_owned(),
            description: "Add two numbers.".to_owned(),
            parameters: transform_anthropic_schema(
                &serde_json::from_str::<Value>(
                    r#"{"title":"x","properties":{"a":{"type":"integer"}},"required":["a"],"type":"object"}"#,
                )
                .unwrap(),
            ),
            strict: None,
        }];
        let history = vec![
            HistoryMessage::Request {
                parts: vec![RequestPart::UserPrompt("hi".to_owned())],
            },
            HistoryMessage::Response {
                parts: vec![
                    ResponsePart::Text("adding".to_owned()),
                    ResponsePart::ToolCall(ToolCall {
                        name: "add".to_owned(),
                        args: ToolArgs::Dict(Map::from_iter([(
                            "a".to_owned(),
                            Value::Number(2.into()),
                        )])),
                        call_id: "toolu_1".to_owned(),
                    }),
                ],
                usage: crate::oss_worker::model_runtime::usage::RequestUsage::zeros(),
                model_name: String::new(),
                finish: Some(Finish::ToolCall),
                provider_details: None,
                response_id: None,
            },
            HistoryMessage::Request {
                parts: vec![RequestPart::ToolReturn {
                    name: "add".to_owned(),
                    content: Value::Number(2.into()),
                    call_id: "toolu_1".to_owned(),
                }],
            },
        ];
        let body = anthropic_request_body("claude-test", "Behave.", &history, &tools, 4096);
        let expected: Value = serde_json::from_str(
            r#"{"max_tokens":4096,"model":"claude-test","messages":[{"role":"user","content":[{"type":"text","text":"hi"}]},{"role":"assistant","content":[{"type":"text","text":"adding"},{"id":"toolu_1","type":"tool_use","name":"add","input":{"a":2}}]},{"role":"user","content":[{"tool_use_id":"toolu_1","type":"tool_result","content":[{"type":"text","text":"2"}],"is_error":false}]}],"stream":false,"tools":[{"name":"add","description":"Add two numbers.","input_schema":{"properties":{"a":{"type":"integer"}},"required":["a"],"type":"object"}}],"tool_choice":{"type":"any"},"system":[{"type":"text","text":"Behave."}]}"#,
        )
        .unwrap();
        assert_eq!(body, expected);
    }

    #[test]
    fn anthropic_forced_choice_rejectors_send_auto() {
        // `supports_forced_tool_choice`
        // (`profiles/anthropic.py:214-216`): the fable/mythos models
        // reject `any`, so forcing degrades to `auto`.
        let history = vec![HistoryMessage::Request {
            parts: vec![RequestPart::UserPrompt("hi".to_owned())],
        }];
        for model in [
            "claude-fable-5-20260101",
            "claude-mythos-5-20260101",
            "claude-mythos-preview-20260101",
        ] {
            let body = anthropic_request_body(model, "", &history, &[], 4096);
            assert_eq!(
                body["tool_choice"]["type"],
                Value::String("auto".to_owned()),
                "{model}"
            );
        }
        let body = anthropic_request_body("claude-opus-4-6", "", &history, &[], 4096);
        assert_eq!(body["tool_choice"]["type"], Value::String("any".to_owned()));
    }

    #[test]
    fn retry_renderings_match_captures() {
        // Unknown-tool retry (`loopprobe.json`): the tool message
        // content is the message plus the fix suffix.
        let text = retry_message(
            &RetryContent::Text(
                "Unknown tool name: 'nope'. Available tools: 'add', 'final_result'".to_owned(),
            ),
            Some("nope"),
        );
        assert_eq!(
            text,
            "Unknown tool name: 'nope'. Available tools: 'add', 'final_result'\n\nFix the errors and try again."
        );
        // Output-validation retry drops top-level `input`
        // (`loopprobe.json` `output_retry_then_ok`).
        let text = retry_message(
            &RetryContent::Errors(vec![
                crate::oss_worker::model_runtime::validation::ValidationDetail {
                    error_type: "missing",
                    loc: vec![crate::oss_worker::model_runtime::validation::LocSeg::Field(
                        "sum".to_owned(),
                    )],
                    msg: "Field required".to_owned(),
                    input: serde_json::from_str::<Value>(r#"{"nope":1}"#).unwrap(),
                },
            ]),
            None,
        );
        assert_eq!(
            text,
            "1 validation error:\n```json\n[\n  {\n    \"type\": \"missing\",\n    \"loc\": [\n      \"sum\"\n    ],\n    \"msg\": \"Field required\"\n  }\n]\n```\n\nFix the errors and try again."
        );
    }

    #[test]
    fn parse_openai_tool_response() {
        let golden = Golden::load().expect("golden parses");
        let table = ProviderTable::load(&golden).expect("table compiles");
        let lane = Lane::OpenAICompatible {
            model: "gpt-4o-mini".to_owned(),
            base_url: "https://api.openai.com/v1".to_owned(),
            api_key: "k".to_owned(),
        };
        let payload: Value = serde_json::from_str(
            r#"{"id":"chatcmpl-x1","object":"chat.completion","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"add","arguments":"{\"a\": 2, \"b\": \"3\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#,
        )
        .unwrap();
        let parsed = parse_openai_response(&lane, &payload, &table).expect("parses");
        assert_eq!(parsed.finish, Some(Finish::ToolCall));
        assert_eq!(parsed.parts.len(), 1);
        let ResponsePart::ToolCall(call) = &parsed.parts[0] else {
            panic!("expected a tool call");
        };
        assert_eq!(call.name, "add");
        assert_eq!(call.call_id, "call_1");
        assert_eq!(call.args_as_json_str(), "{\"a\": 2, \"b\": \"3\"}");
        assert_eq!(
            call.args_as_dict(),
            Map::from_iter([
                ("a".to_owned(), Value::Number(2.into())),
                ("b".to_owned(), Value::String("3".to_owned())),
            ])
        );
        assert_eq!(parsed.model_name, "gpt-4o-mini");
        assert_eq!(parsed.response_id.as_deref(), Some("chatcmpl-x1"));
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
    fn empty_tool_ids_regenerate() {
        // `t.tool_call_id or generate_tool_call_id()`
        // (`_utils.py:419`): empty ids regenerate like missing ones.
        let golden = Golden::load().expect("golden parses");
        let table = ProviderTable::load(&golden).expect("table compiles");
        let openai = Lane::OpenAICompatible {
            model: "m".to_owned(),
            base_url: "https://t/v1".to_owned(),
            api_key: "k".to_owned(),
        };
        let payload: Value = serde_json::from_str(
            r#"{"id":"c","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{"id":"","type":"function","function":{"name":"add","arguments":"{}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#,
        )
        .unwrap();
        let parsed = parse_openai_response(&openai, &payload, &table).expect("parses");
        let ResponsePart::ToolCall(call) = &parsed.parts[0] else {
            panic!("expected a tool call");
        };
        assert!(call.call_id.starts_with("pyd_ai_"), "got: {}", call.call_id);
        let anthropic = Lane::Anthropic {
            model: "c".to_owned(),
            api_key: "k".to_owned(),
        };
        let payload: Value = serde_json::from_str(
            r#"{"id":"m","model":"c","stop_reason":"tool_use","content":[{"type":"tool_use","id":"","name":"add","input":{}}],"usage":{"input_tokens":1,"output_tokens":1}}"#,
        )
        .unwrap();
        let parsed = parse_anthropic_response(&anthropic, &payload, &table).expect("parses");
        let ResponsePart::ToolCall(call) = &parsed.parts[0] else {
            panic!("expected a tool call");
        };
        assert!(call.call_id.starts_with("pyd_ai_"), "got: {}", call.call_id);
    }

    #[test]
    fn missing_created_backfills_timestamp() {
        // `if not response.created: ... = int(now)`
        // (`openai.py:1068-1069`): the `timestamp` detail is always
        // present, on both the refusal and the normal arms.
        let golden = Golden::load().expect("golden parses");
        let table = ProviderTable::load(&golden).expect("table compiles");
        let lane = Lane::OpenAICompatible {
            model: "m".to_owned(),
            base_url: "https://t/v1".to_owned(),
            api_key: "k".to_owned(),
        };
        for payload in [
            r#"{"id":"c","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":null,"refusal":"nope"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#,
            r#"{"id":"c","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"x"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#,
        ] {
            let payload: Value = serde_json::from_str(payload).unwrap();
            let parsed = parse_openai_response(&lane, &payload, &table).expect("parses");
            let details = parsed.provider_details.expect("details");
            let timestamp = details
                .get("timestamp")
                .and_then(Value::as_str)
                .expect("timestamp");
            assert!(timestamp.ends_with('Z'), "got: {timestamp}");
            assert!(timestamp > "2026-01-01T00:00:00Z");
        }
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
    fn parse_openai_refusal_and_edges() {
        let golden = Golden::load().expect("golden parses");
        let table = ProviderTable::load(&golden).expect("table compiles");
        let lane = Lane::OpenAICompatible {
            model: "m".to_owned(),
            base_url: "https://t/v1".to_owned(),
            api_key: "k".to_owned(),
        };
        // Refusal short-circuits with empty parts.
        let payload: Value = serde_json::from_str(
            r#"{"id":"c","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":null,"refusal":"nope"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#,
        )
        .unwrap();
        let parsed = parse_openai_response(&lane, &payload, &table).expect("parses");
        assert_eq!(parsed.finish, Some(Finish::ContentFilter));
        assert!(parsed.parts.is_empty());
        // Ollama's null finish reason reads as stop.
        let payload: Value = serde_json::from_str(
            r#"{"id":"c","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"x"},"finish_reason":null}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#,
        )
        .unwrap();
        let parsed = parse_openai_response(&lane, &payload, &table).expect("parses");
        assert_eq!(parsed.finish, Some(Finish::Stop));
        // Unknown reasons map to None; parts still stand.
        let payload: Value = serde_json::from_str(
            r#"{"id":"c","model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"x"},"finish_reason":"weird"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#,
        )
        .unwrap();
        let parsed = parse_openai_response(&lane, &payload, &table).expect("parses");
        assert_eq!(parsed.finish, None);
        assert_eq!(parsed.parts.len(), 1);
        // Missing choices is an invalid response.
        let payload: Value = serde_json::from_str(r#"{"id":"c"}"#).unwrap();
        let error = parse_openai_response(&lane, &payload, &table).unwrap_err();
        assert!(matches!(error, ProviderError::InvalidResponse(_)));
    }

    #[test]
    fn parse_anthropic_message_response() {
        let golden = Golden::load().expect("golden parses");
        let table = ProviderTable::load(&golden).expect("table compiles");
        let lane = Lane::Anthropic {
            model: "claude-test".to_owned(),
            api_key: "k".to_owned(),
        };
        let payload: Value = serde_json::from_str(
            r#"{"id":"msg_x1","type":"message","role":"assistant","model":"claude-test","stop_reason":"tool_use","content":[{"type":"text","text":"adding"},{"type":"tool_use","id":"toolu_1","name":"add","input":{"a":2}}],"usage":{"input_tokens":10,"output_tokens":5}}"#,
        )
        .unwrap();
        let parsed = parse_anthropic_response(&lane, &payload, &table).expect("parses");
        assert_eq!(parsed.finish, Some(Finish::ToolCall));
        assert_eq!(parsed.parts.len(), 2);
        assert!(matches!(parsed.parts[0], ResponsePart::Text(_)));
        // max_tokens maps to length.
        let payload: Value = serde_json::from_str(
            r#"{"id":"m","model":"c","stop_reason":"max_tokens","content":[],"usage":{"input_tokens":1,"output_tokens":1}}"#,
        )
        .unwrap();
        let parsed = parse_anthropic_response(&lane, &payload, &table).expect("parses");
        assert_eq!(parsed.finish, Some(Finish::Length));
        assert!(parsed.parts.is_empty());
        // refusal maps to content_filter with the explanation.
        let payload: Value = serde_json::from_str(
            r#"{"id":"m","model":"c","stop_reason":"refusal","stop_details":{"explanation":"bad","category":"weapons"},"content":[],"usage":{"input_tokens":1,"output_tokens":1}}"#,
        )
        .unwrap();
        let parsed = parse_anthropic_response(&lane, &payload, &table).expect("parses");
        assert_eq!(parsed.finish, Some(Finish::ContentFilter));
        let details = parsed.provider_details.expect("details");
        assert_eq!(details.get("refusal").and_then(Value::as_str), Some("bad"));
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

    #[test]
    fn error_body_strips_before_parsing() {
        // `err_text = response.text.strip()`, then parse-or-keep.
        let (text, json) = parse_error_body(b"upstream exploded\n\n");
        assert_eq!(text, "upstream exploded");
        assert_eq!(json, None);
        let (text, json) = parse_error_body(b"  {\"a\": 1}  \n");
        assert_eq!(text, "{\"a\": 1}");
        assert_eq!(json, Some(serde_json::from_str(r#"{"a": 1}"#).unwrap()));
        let (text, json) = parse_error_body(b"");
        assert_eq!(text, "");
        assert_eq!(json, None);
    }

    fn thinking_lane() -> Lane {
        Lane::OpenAICompatible {
            model: "m".to_owned(),
            base_url: "https://t/v1".to_owned(),
            api_key: "k".to_owned(),
        }
    }

    fn parse_message(message: &str) -> ParsedResponse {
        let golden = Golden::load().expect("golden parses");
        let table = ProviderTable::load(&golden).expect("table compiles");
        let payload: Value = serde_json::from_str(&format!(
            r#"{{"id":"c","model":"m","choices":[{{"index":0,"message":{message},"finish_reason":"stop"}}],"usage":{{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}}}"#,
        ))
        .unwrap();
        parse_openai_response(&thinking_lane(), &payload, &table).expect("parses")
    }

    fn thinking_parts(parts: &[ResponsePart]) -> Vec<(&str, &str)> {
        parts
            .iter()
            .map(|part| match part {
                ResponsePart::Text(text) => ("text", text.as_str()),
                ResponsePart::Thinking { id, content } => (id.as_str(), content.as_str()),
                ResponsePart::ToolCall(call) => ("tool", call.name.as_str()),
            })
            .collect()
    }

    #[test]
    fn openai_thinking_fields_map_first_non_empty() {
        // `_process_thinking` (`openai.py:1160-1176`): `reasoning`
        // wins over `reasoning_content`; empty and non-string values
        // fall through, and field-sourced thinking lands first.
        let parsed = parse_message(
            r#"{"role":"assistant","content":"hi","reasoning":"r1","reasoning_content":"rc"}"#,
        );
        assert_eq!(
            thinking_parts(&parsed.parts),
            vec![("reasoning", "r1"), ("text", "hi")]
        );
        let parsed = parse_message(r#"{"role":"assistant","content":"","reasoning_content":"rc"}"#);
        assert_eq!(
            thinking_parts(&parsed.parts),
            vec![("reasoning_content", "rc")]
        );
        let parsed = parse_message(
            r#"{"role":"assistant","content":null,"reasoning":"","reasoning_content":"rc"}"#,
        );
        assert_eq!(
            thinking_parts(&parsed.parts),
            vec![("reasoning_content", "rc")]
        );
        let parsed = parse_message(
            r#"{"role":"assistant","content":null,"reasoning":{"deep":true},"reasoning_content":"rc"}"#,
        );
        assert_eq!(
            thinking_parts(&parsed.parts),
            vec![("reasoning_content", "rc")]
        );
        let parsed = parse_message(r#"{"role":"assistant","content":"hi"}"#);
        assert_eq!(thinking_parts(&parsed.parts), vec![("text", "hi")]);
        // Non-string reasoning with nothing after it: no thinking part.
        let parsed = parse_message(r#"{"role":"assistant","content":null,"reasoning":["x"]}"#);
        assert!(parsed.parts.is_empty());
    }

    #[test]
    fn content_think_tags_split() {
        // `split_content_into_text_and_thinking`
        // (`_thinking_part.py:6-31`): tag-sourced thinking carries
        // `id: "content"`, empty think spans still emit, an unclosed
        // tag folds the rest into text, and empty text spans emit
        // nothing.
        assert_eq!(
            thinking_parts(&split_content_into_text_and_thinking(
                "<think>plan</think>{\"a\":1}"
            )),
            vec![("content", "plan"), ("text", "{\"a\":1}")]
        );
        assert_eq!(
            thinking_parts(&split_content_into_text_and_thinking("a<think>b</think>c")),
            vec![("text", "a"), ("content", "b"), ("text", "c")]
        );
        assert_eq!(
            thinking_parts(&split_content_into_text_and_thinking("<think></think>")),
            vec![("content", "")]
        );
        assert_eq!(
            thinking_parts(&split_content_into_text_and_thinking("<think>abc")),
            vec![("text", "abc")]
        );
        assert_eq!(
            thinking_parts(&split_content_into_text_and_thinking("plain")),
            vec![("text", "plain")]
        );
        assert!(split_content_into_text_and_thinking("").is_empty());
        // Through the full parse: tags split and the JSON validates
        // downstream (the issue's case (a)).
        let parsed =
            parse_message(r#"{"role":"assistant","content":"<think>plan</think>{\"a\":1}"}"#);
        assert_eq!(
            thinking_parts(&parsed.parts),
            vec![("content", "plan"), ("text", "{\"a\":1}")]
        );
    }

    #[test]
    fn openai_assistant_message_replays_thinking() {
        // `auto` mode (`openai.py:1363-1400`): field-sourced thinking
        // replays as `message[field]` with `content: null` when
        // textless; tag-sourced thinking replays as a `<think>` block
        // inside the texts, in order; a thinking-only response still
        // emits a message, while a partless one emits nothing.
        let field = openai_assistant_message(&[ResponsePart::Thinking {
            id: "reasoning_content".to_owned(),
            content: "plan".to_owned(),
        }])
        .expect("thinking emits a message");
        assert_eq!(
            field,
            serde_json::json!({
                "role": "assistant",
                "reasoning_content": "plan",
                "content": null,
            })
        );
        assert_eq!(
            serde_json::to_string(&field).unwrap(),
            r#"{"role":"assistant","reasoning_content":"plan","content":null}"#,
        );
        let mixed = openai_assistant_message(&[
            ResponsePart::Text("a".to_owned()),
            ResponsePart::Thinking {
                id: "content".to_owned(),
                content: "b".to_owned(),
            },
            ResponsePart::Text("c".to_owned()),
        ])
        .expect("mixed emits a message");
        assert_eq!(
            mixed["content"],
            Value::String("a\n\n<think>\nb\n</think>\n\nc".to_owned())
        );
        assert!(mixed.get("reasoning_content").is_none());
        let empty: Vec<ResponsePart> = Vec::new();
        assert!(openai_assistant_message(&empty).is_none());
    }

    #[test]
    fn openai_empty_tool_result_is_empty_string() {
        // `_unwrap_data` (`messages.py:1179-1187`) returns None for
        // empty data, so `model_response_str` (`messages.py:1243-1250`)
        // renders an empty list or null as `''` — not `"[]"`/`"null"`.
        // Non-empty lists still render as compact JSON.
        for (content, expected) in [
            (Value::Array(Vec::new()), ""),
            (Value::Null, ""),
            (serde_json::json!([{"id": "1"}]), r#"[{"id":"1"}]"#),
        ] {
            let history = vec![HistoryMessage::Request {
                parts: vec![RequestPart::ToolReturn {
                    name: "pidash_search_project_issues".to_owned(),
                    content,
                    call_id: "call_1".to_owned(),
                }],
            }];
            let body = openai_request_body("gpt-4o-mini", "", &history, &[], 0);
            assert_eq!(
                body["messages"],
                serde_json::json!([
                    {"role": "tool", "tool_call_id": "call_1", "content": expected},
                ])
            );
        }
    }
}
