//! Provider streaming for assistant turns (PIDASHCONV-1111, stage 8).
//!
//! Ports the provider-facing half of `assistant.run_turn`: the model
//! configuration (`runtime/llm.py`, BYOK branch + key) plus the
//! pydantic-ai 1.107 model clients (`models/openai.py`
//! `OpenAIChatModel` / `models/anthropic.py` `AnthropicModel`) in the
//! exact configuration the turn exercises: streaming requests, text
//! output, function tools, no model settings, default profiles.
//!
//! * Request building ([`openai_request_body`], [`anthropic_request_body`])
//!   over the history codec's [`TurnMessage`][super::history::TurnMessage]
//!   plus the D-11 [`WireTool`][provider::WireTool] list. [`encode_body`]
//!   serializes to the exact bytes the Python SDKs send (compact UTF-8,
//!   SDK key order), pinned by the `turn-runtime` captures.
//! * SSE parsing ([`OpenAIStream`], [`AnthropicStream`]): incremental
//!   feeds yielding live [`StreamEvent`] text deltas for the turn
//!   streamer (`tasks.py` `_Streamer.handle`, which reads `TextPart`
//!   starts and `TextPartDelta` deltas only) and assembling the final
//!   [`StreamedTurn`] (parts, usage, finish, provider stamps) exactly
//!   `StreamedResponse.get()` would.
//! * The streaming HTTP client ([`stream_turn`]): reqwest POST with the
//!   stainless retry loop for the initial request, then chunk reads
//!   through the lane parser.
//! * Error mapping ([`classify_provider_error`]): the D-11
//!   [`ProviderError`][provider::ProviderError] message plus its HTTP
//!   status, structured-body one-liner and lane model name into
//!   [`classify_provider_error`][pidash_jobs::assistant::classify_provider_error]
//!   (`tasks.py` `_classify_error`), so 401/402/429/timeout failures
//!   record the same codes Django records.
//! * Usage accounting ([`add_request_usage`] plus the per-lane
//!   extractors): OpenAI per-chunk accumulation, Anthropic
//!   start/replace-with-merged-details.
//!
//! Reuse rule: everything `pub` and constructible in the D-11 runtime
//! is reused (`Lane`, `WireTool`, `ToolArgs`, `ToolCall`,
//! `ProviderError`, usage, `generate_tool_call_id`, `py_repr`). The
//! D-11 request builders and send loop are not reusable from the
//! binary (crate-private history fields; a private non-streaming
//! sender), so the turn-config equivalents live here, each citing the
//! same SDK lines. The agent loop consumes this module (PIDASHCONV-863);
//! nothing here touches the DB, Redis, or the worker registry.
//!
//! Turn-subset scope (each cites why the rest is unreachable):
//!
//! * No thinking/reasoning parts: the turn never sets thinking effort
//!   or reasoning fields, so no `ThinkingPart` is ever produced or
//!   replayed. The default profile's `<think>` tags
//!   (`profiles/__init__.py:125`) would split a text chunk equal to
//!   the bare tag into a thinking part; that content rides as text
//!   here instead (content and order preserved, part kind differs).
//!   Listed in the PR.
//! * No native/server tools, files, images, cache control, MCP wire
//!   tools, compaction, or logprobs: none are configured on the turn
//!   path. Unknown Anthropic blocks/deltas are ignored exactly like
//!   the `elif` chain with no `else` (`anthropic.py:2200-2290`).
//! * No `strict` inference: the builder emits the `strict` key
//!   exactly when the incoming [`WireTool`][provider::WireTool] sets
//!   it (some OpenAI-lane captures do); the inference itself belongs
//!   to the schema layer (PIDASHCONV-1113).

use pidash_jobs::oss_worker::model_runtime::provider::{
    generate_tool_call_id, py_repr, Finish, Lane, ProviderError, ToolArgs, WireTool,
    ANTHROPIC_VERSION, USER_AGENT,
};
use pidash_jobs::oss_worker::model_runtime::usage::{ProviderTable, RequestUsage};
use pidash_services::assistant::retry::{should_retry_status, sleep_for_retry, MAX_RETRIES};
use serde_json::{Map, Value};
use std::collections::HashMap;

use super::history::{retry_message, TurnMessage, TurnRequestPart, TurnResponsePart};

// ---------------------------------------------------------------------------
// Request building
// ---------------------------------------------------------------------------

/// Anthropic `max_tokens` when the turn sets none
/// (`_messages_create`, `anthropic.py:714`).
pub const ANTHROPIC_DEFAULT_MAX_TOKENS: i64 = 4096;

/// Build the OpenAI chat-completions streaming body
/// (`_completions_create` / `_map_messages`,
/// `openai.py:970-1040,1445-1494`) for the turn configuration:
/// `{messages, model, stream: true, stream_options: {include_usage:
/// true}, tool_choice: "auto", tools}` (SDK key order, pinned by the
/// captures). No `max_tokens` (the turn sets no model settings, so
/// `max_completion_tokens` is `OMIT`), no `parallel_tool_calls`,
/// `response_format`, or sampling keys (all `OMIT` without settings).
///
/// `instructions` are the agent instruction parts in order (the
/// stripped static literal, then each dynamic instruction,
/// `agent/__init__.py:2588,1477-1485`); each becomes its own system
/// message. `history` is the loop's messages (fresh prompt plus
/// replayed turns); `tools` the wire tools for this request.
pub fn openai_request_body(
    model: &str,
    instructions: &[String],
    history: &[TurnMessage],
    tools: &[WireTool],
) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    for message in history {
        match message {
            TurnMessage::Request { parts, .. } => {
                for part in parts {
                    match part {
                        TurnRequestPart::User { text, .. } => {
                            messages.push(openai_user_message(text));
                        }
                        TurnRequestPart::ToolReturn {
                            content, call_id, ..
                        } => {
                            messages.push(Value::Object(Map::from_iter([
                                ("role".to_owned(), Value::String("tool".to_owned())),
                                (
                                    "tool_call_id".to_owned(),
                                    Value::String(guard_tool_call_id(call_id)),
                                ),
                                (
                                    "content".to_owned(),
                                    Value::String(tool_response_str(content)),
                                ),
                            ])));
                        }
                        TurnRequestPart::Retry {
                            tool_name,
                            content,
                            call_id,
                            ..
                        } => {
                            let text = retry_message(content, tool_name.as_deref());
                            if tool_name.is_none() {
                                messages.push(openai_user_message(&text));
                            } else {
                                messages.push(Value::Object(Map::from_iter([
                                    ("role".to_owned(), Value::String("tool".to_owned())),
                                    (
                                        "tool_call_id".to_owned(),
                                        Value::String(guard_tool_call_id(call_id)),
                                    ),
                                    ("content".to_owned(), Value::String(text)),
                                ])));
                            }
                        }
                    }
                }
            }
            TurnMessage::Response { parts, .. } => {
                if let Some(mapped) = openai_assistant_message(parts) {
                    messages.push(mapped);
                }
            }
        }
    }
    if !instructions.is_empty() {
        // `openai_system_prompt_role` (`profiles/openai.py:218`):
        // o1-mini rejects the `system` role, so its instructions
        // travel as `user`. Either way they insert after the leading
        // same-role messages (always none here: no `SystemPromptPart`
        // survives the codec, so no system/user message precedes that
        // a system insert must skip — for the o1-mini user-role case
        // the count covers leading user prompts, like Python).
        let role = if model.starts_with("o1-mini") {
            "user"
        } else {
            "system"
        };
        let at = messages
            .iter()
            .take_while(|message| message.get("role").and_then(Value::as_str) == Some(role))
            .count();
        let instruction_messages: Vec<Value> = instructions
            .iter()
            .map(|content| openai_role_message(role, content))
            .collect();
        messages.splice(at..at, instruction_messages);
    }
    // No `_merge_leading_system_messages`: the default profile
    // supports multiple system messages (pinned: the captures carry
    // two), so the merge never runs on the turn path.
    let wire_tools: Vec<Value> = tools.iter().map(openai_wire_tool).collect();
    let mut body = Map::from_iter([
        ("messages".to_owned(), Value::Array(messages)),
        ("model".to_owned(), Value::String(model.to_owned())),
        ("stream".to_owned(), Value::Bool(true)),
        (
            "stream_options".to_owned(),
            Value::Object(Map::from_iter([(
                "include_usage".to_owned(),
                Value::Bool(true),
            )])),
        ),
        ("tool_choice".to_owned(), Value::String("auto".to_owned())),
    ]);
    // `tools or OMIT`: the turn always sends tools, but an empty set
    // omits the key exactly like the SDK call.
    if !wire_tools.is_empty() {
        body.insert("tools".to_owned(), Value::Array(wire_tools));
    }
    Value::Object(body)
}

fn openai_user_message(content: &str) -> Value {
    openai_role_message("user", content)
}

fn openai_role_message(role: &str, content: &str) -> Value {
    Value::Object(Map::from_iter([
        ("role".to_owned(), Value::String(role.to_owned())),
        ("content".to_owned(), Value::String(content.to_owned())),
    ]))
}

/// The assistant replay (`_MapModelResponseContext`,
/// `openai.py:1293-1363`): texts joined with `\n\n`, `content: null`
/// without texts, tool calls in order; `None` when there is nothing
/// to send.
fn openai_assistant_message(parts: &[TurnResponsePart]) -> Option<Value> {
    let mut texts: Vec<&str> = Vec::new();
    let mut calls: Vec<Value> = Vec::new();
    for part in parts {
        match part {
            TurnResponsePart::Text { text, .. } => texts.push(text.as_str()),
            TurnResponsePart::ToolCall {
                name,
                args,
                call_id,
                ..
            } => {
                calls.push(Value::Object(Map::from_iter([
                    ("id".to_owned(), Value::String(guard_tool_call_id(call_id))),
                    ("type".to_owned(), Value::String("function".to_owned())),
                    (
                        "function".to_owned(),
                        Value::Object(Map::from_iter([
                            ("name".to_owned(), Value::String(name.to_owned())),
                            (
                                "arguments".to_owned(),
                                Value::String(tool_args_json_str(args.as_ref())),
                            ),
                        ])),
                    ),
                ])));
            }
        }
    }
    if texts.is_empty() && calls.is_empty() {
        return None;
    }
    let mut message = Map::from_iter([("role".to_owned(), Value::String("assistant".to_owned()))]);
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

/// `_map_tool_definition` (`openai.py:1507-1523`): `{type, function:
/// {name, description, parameters}}`, plus `strict` only when the
/// definition infers it (never on the turn path).
fn openai_wire_tool(tool: &WireTool) -> Value {
    let mut function = Map::from_iter([
        ("name".to_owned(), Value::String(tool.name.clone())),
        (
            "description".to_owned(),
            Value::String(tool.description.clone()),
        ),
        ("parameters".to_owned(), tool.parameters.clone()),
    ]);
    if tool.strict == Some(true) {
        function.insert("strict".to_owned(), Value::Bool(true));
    }
    Value::Object(Map::from_iter([
        ("type".to_owned(), Value::String("function".to_owned())),
        ("function".to_owned(), Value::Object(function)),
    ]))
}

/// Build the Anthropic messages streaming body (`_messages_create` /
/// `_map_message`, `anthropic.py:682-740,1202-1622`) for the turn
/// configuration: `{max_tokens: 4096, messages, model, stream: true,
/// system, tool_choice: {type: "auto"}, tools}` (SDK key order,
/// pinned by the captures). Each instruction part becomes its own
/// system text block; no betas, cache control, thinking, or output
/// config (all unset without model settings).
pub fn anthropic_request_body(
    model: &str,
    instructions: &[String],
    history: &[TurnMessage],
    tools: &[WireTool],
) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    for message in history {
        match message {
            TurnMessage::Request { parts, .. } => {
                let mut blocks: Vec<Value> = Vec::new();
                for part in parts {
                    match part {
                        TurnRequestPart::User { text, .. } => {
                            // Empty text is skipped (`_map_user_prompt`,
                            // `anthropic.py:1892-1900`).
                            if !text.is_empty() {
                                blocks.push(anthropic_text_block_text_first(text));
                            }
                        }
                        TurnRequestPart::ToolReturn {
                            content, call_id, ..
                        } => {
                            blocks.push(anthropic_tool_result(call_id, content));
                        }
                        TurnRequestPart::Retry {
                            tool_name,
                            content,
                            call_id,
                            ..
                        } => {
                            let text = retry_message(content, tool_name.as_deref());
                            if tool_name.is_none() {
                                blocks.push(anthropic_text_block_type_first(&text));
                            } else {
                                blocks.push(Value::Object(Map::from_iter([
                                    (
                                        "tool_use_id".to_owned(),
                                        Value::String(guard_tool_call_id(call_id)),
                                    ),
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
            TurnMessage::Response { parts, .. } => {
                let mut blocks: Vec<Value> = Vec::new();
                for part in parts {
                    match part {
                        TurnResponsePart::Text { text, .. } => {
                            if !text.is_empty() {
                                blocks.push(anthropic_text_block_text_first(text));
                            }
                        }
                        TurnResponsePart::ToolCall {
                            name,
                            args,
                            call_id,
                            ..
                        } => {
                            blocks.push(Value::Object(Map::from_iter([
                                ("id".to_owned(), Value::String(guard_tool_call_id(call_id))),
                                ("type".to_owned(), Value::String("tool_use".to_owned())),
                                ("name".to_owned(), Value::String(name.to_owned())),
                                (
                                    "input".to_owned(),
                                    Value::Object(tool_args_dict(args.as_ref())),
                                ),
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
    // `supports_forced_tool_choice` only matters for `required`; the
    // turn resolves `auto`, so the choice is always `{type: "auto"}`
    // with no `disable_parallel_tool_use` (no `parallel_tool_calls`
    // setting, `anthropic.py:1197-1198`).
    let mut body = Map::from_iter([
        (
            "max_tokens".to_owned(),
            Value::Number(ANTHROPIC_DEFAULT_MAX_TOKENS.into()),
        ),
        ("messages".to_owned(), Value::Array(messages)),
        ("model".to_owned(), Value::String(model.to_owned())),
        ("stream".to_owned(), Value::Bool(true)),
    ]);
    // `system=system_prompt or OMIT`: no `SystemPromptPart` survives
    // the codec, so the system is exactly the instruction blocks.
    if !instructions.is_empty() {
        let blocks: Vec<Value> = instructions
            .iter()
            .map(|content| anthropic_text_block_type_first(content))
            .collect();
        body.insert("system".to_owned(), Value::Array(blocks));
    }
    body.insert(
        "tool_choice".to_owned(),
        Value::Object(Map::from_iter([(
            "type".to_owned(),
            Value::String("auto".to_owned()),
        )])),
    );
    // `tools or OMIT`, like the OpenAI lane.
    if !wire_tools.is_empty() {
        body.insert("tools".to_owned(), Value::Array(wire_tools));
    }
    Value::Object(body)
}

/// `BetaTextBlockParam(text=..., type='text')` call order
/// (`_map_user_prompt`, tool-result items, assistant texts): `text`
/// first. The order is load-bearing for byte identity.
fn anthropic_text_block_text_first(text: &str) -> Value {
    Value::Object(Map::from_iter([
        ("text".to_owned(), Value::String(text.to_owned())),
        ("type".to_owned(), Value::String("text".to_owned())),
    ]))
}

/// `BetaTextBlockParam(type='text', text=...)` call order (system
/// blocks, output-retry text): `type` first.
fn anthropic_text_block_type_first(text: &str) -> Value {
    Value::Object(Map::from_iter([
        ("type".to_owned(), Value::String("text".to_owned())),
        ("text".to_owned(), Value::String(text.to_owned())),
    ]))
}

/// A success tool result: one text block per content item
/// (`content_items(mode='str')` — strings verbatim, anything else as
/// compact JSON), or `""` when there is no content
/// (`anthropic.py:1266-1306`).
fn anthropic_tool_result(call_id: &str, content: &Value) -> Value {
    let items: Vec<Value> = match content {
        // A JSON null is one item (`[None]`), rendered as the string
        // `"null"` (`tool_return_ta.dump_json(None)`); only an empty
        // list yields zero items.
        Value::Array(items) if items.is_empty() => Vec::new(),
        Value::Array(items) => items.iter().map(tool_result_item_text).collect(),
        single => vec![tool_result_item_text(single)],
    }
    .into_iter()
    .map(|text| anthropic_text_block_text_first(&text))
    .collect();
    // `content or ''`: an empty item list sends the empty string.
    let content = if items.is_empty() {
        Value::String(String::new())
    } else {
        Value::Array(items)
    };
    Value::Object(Map::from_iter([
        (
            "tool_use_id".to_owned(),
            Value::String(guard_tool_call_id(call_id)),
        ),
        ("type".to_owned(), Value::String("tool_result".to_owned())),
        ("content".to_owned(), content),
        ("is_error".to_owned(), Value::Bool(false)),
    ]))
}

/// One `content_items(mode='str')` item: strings verbatim, anything
/// else as compact JSON (`messages.py:1209-1241`). No value the codec
/// stores is multimodal content (those are Python-only dataclass
/// instances, never turn tool results), so every item serializes.
fn tool_result_item_text(item: &Value) -> String {
    match item {
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or("null".to_owned()),
    }
}

/// `_map_tool_definition` (`anthropic.py:1933-1950`):
/// `{name, description, input_schema}`; `strict` only when the
/// definition sets it (never on the turn path), no eager-streaming
/// or `defer_loading` keys (no such settings or native tools).
fn anthropic_wire_tool(tool: &WireTool) -> Value {
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

/// `guard_tool_call_id` (`_utils.py:411-419`): the stored id, or a
/// fresh `pyd_ai_{uuid4hex}` when empty (stored ids are never empty
/// — the codec generates on absence — so the fallback only fires for
/// loop-built parts that skipped it).
fn guard_tool_call_id(call_id: &str) -> String {
    if call_id.is_empty() {
        generate_tool_call_id()
    } else {
        call_id.to_owned()
    }
}

/// Render one tool-return value for the OpenAI lane
/// (`model_response_str`, `messages.py:1243-1252`): null and the
/// empty list read as `""` (`_unwrap_data` yields `None`); a string
/// verbatim; anything else — including a non-empty list, which stays
/// a list — as compact JSON.
fn tool_response_str(content: &Value) -> String {
    match content {
        Value::Null => String::new(),
        Value::Array(items) if items.is_empty() => String::new(),
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or("null".to_owned()),
    }
}

/// `args_as_json_str()` (`messages.py:1851-1860`): falsy args (none
/// or the empty string) replay as `'{}'`; raw strings replay
/// verbatim (the captures keep the provider's own spacing); dicts
/// serialize compact.
fn tool_args_json_str(args: Option<&ToolArgs>) -> String {
    match args {
        None => "{}".to_owned(),
        Some(ToolArgs::Raw(raw)) if raw.is_empty() => "{}".to_owned(),
        Some(ToolArgs::Raw(raw)) => raw.clone(),
        Some(ToolArgs::Dict(map)) => {
            serde_json::to_string(&Value::Object(map.clone())).unwrap_or("{}".to_owned())
        }
    }
}

/// `args_as_dict()` (`messages.py:1838-1847`): falsy args (none or
/// the empty string) read `{}`, dicts as-is; strings parsed, falling
/// back to the `INVALID_JSON` wrap when the payload is not a JSON
/// object.
fn tool_args_dict(args: Option<&ToolArgs>) -> Map<String, Value> {
    match args {
        None => Map::new(),
        Some(ToolArgs::Dict(map)) => map.clone(),
        Some(ToolArgs::Raw(raw)) if raw.is_empty() => Map::new(),
        Some(ToolArgs::Raw(raw)) => match serde_json::from_str::<Value>(raw) {
            Ok(Value::Object(map)) => map,
            _ => Map::from_iter([(
                pidash_jobs::oss_worker::model_runtime::provider::INVALID_JSON_KEY.to_owned(),
                Value::String(raw.clone()),
            )]),
        },
    }
}

/// Serialize a request body to the exact bytes the Python SDKs send:
/// compact JSON with unescaped Unicode
/// (`json.dumps(separators=(',',':'), ensure_ascii=False)`), which is
/// exactly `serde_json::to_vec` over the insertion-ordered body. The
/// replay tests pin every exchange byte for byte.
pub fn encode_body(body: &Value) -> Vec<u8> {
    serde_json::to_vec(body).unwrap_or_default()
}

/// The streaming endpoint for a lane: the Anthropic SDK beta client
/// posts `/v1/messages?beta=true`
/// (`anthropic/resources/beta/messages/messages.py:1242`, pinned by
/// the captures' request paths); the OpenAI lane posts the
/// configured chat-completions URL. Tests pass their stub URL
/// directly to [`stream_turn`] instead.
pub fn stream_endpoint(lane: &Lane) -> String {
    match lane {
        Lane::Anthropic { .. } => format!("{}?beta=true", lane.endpoint_url()),
        Lane::OpenAICompatible { .. } => lane.endpoint_url(),
    }
}

// ---------------------------------------------------------------------------
// Streaming events and results
// ---------------------------------------------------------------------------

/// One live event from the provider stream: the `TextPart` starts
/// and `TextPartDelta` deltas the turn streamer consumes
/// (`tasks.py` `_Streamer.handle`). `index` is the part index in the
/// final parts list, like `PartStartEvent`/`PartDeltaEvent`.
/// Tool-call assembly stays internal: calls surface complete in
/// [`StreamedTurn::parts`], exactly what `get()` returns after the
/// stream is exhausted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    /// A new text part started; `initial` is its first content (may
    /// be empty when the part starts structurally, e.g. an Anthropic
    /// text block with later deltas — the manager still emits the
    /// start on the first delta).
    TextStart { index: usize, initial: String },
    /// More content for the text part at `index`.
    TextDelta { index: usize, delta: String },
}

/// One streamed tool call: `ToolCallPart` with the streamed-args
/// shape — `None` when an Anthropic `tool_use` block yields zero
/// `input_json` deltas (`anthropic.py:2222`, `... or None`). There is
/// no `None` arm on the D-11 `ToolCall`, so the stream carries its
/// own part type; the loop maps it onto `TurnResponsePart::ToolCall`
/// field for field.
#[derive(Debug, Clone)]
pub struct StreamToolCall {
    pub name: String,
    pub args: Option<ToolArgs>,
    pub call_id: String,
}

/// One response part assembled from the stream (`TextPart` /
/// `ToolCallPart`; ids and provider stamps are `None` on the turn
/// path — neither lane passes them for these parts).
#[derive(Debug, Clone)]
pub enum StreamPart {
    Text(String),
    ToolCall(StreamToolCall),
}

/// The assembled stream result: what `StreamedResponse.get()`
/// returns after exhaustion (`models/__init__.py:875-895`) — parts
/// in creation order (incomplete tool deltas excluded, like
/// `get_parts`), the accumulated usage, the mapped finish reason,
/// the provider's model name / response id / details. The timestamp
/// is volatile (always now) and left to the loop's message
/// constructor.
#[derive(Debug, Clone)]
pub struct StreamedTurn {
    pub parts: Vec<StreamPart>,
    pub usage: RequestUsage,
    pub finish: Option<Finish>,
    pub model_name: String,
    pub response_id: Option<String>,
    pub provider_details: Option<Map<String, Value>>,
}

/// A vendor part id: the key the parts manager tracks deltas under
/// (`_parts_manager.py`). OpenAI text shares the single `'content'`
/// id; tool calls use the chunk's `index`; Anthropic uses the block
/// `index` for both.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum VendorId {
    Content,
    Index(i64),
}

/// One managed part slot: text accumulates a string; tool calls
/// accumulate name/args/id fragments until the name lands (a slot
/// without a name is a `ToolCallPartDelta`, excluded from the final
/// parts like `get_parts`, `messages.py:2543-2620`).
#[derive(Debug, Clone)]
enum PartSlot {
    Text(String),
    ToolDelta {
        name: Option<String>,
        args: Option<ToolArgs>,
        call_id: Option<String>,
    },
}

/// Shared parts-manager state (`ModelResponsePartsManager`,
/// `_parts_manager.py:58-120`): slots in creation order plus the
/// vendor-id index. Typed-call promotion is a no-op here (turn tool
/// defs carry no `tool_kind`).
#[derive(Debug, Default)]
struct PartsManager {
    parts: Vec<PartSlot>,
    by_vendor: HashMap<VendorId, usize>,
}

impl PartsManager {
    /// `handle_text_delta` (`_parts_manager.py:121-207`) for the
    /// turn subset: append to the vendor part, or start a new text
    /// part. Applying text to a tool slot raises, like Python.
    fn text_delta(
        &mut self,
        vendor: VendorId,
        content: &str,
    ) -> Result<StreamEvent, ProviderError> {
        if let Some(&index) = self.by_vendor.get(&vendor) {
            match &mut self.parts[index] {
                PartSlot::Text(text) => {
                    text.push_str(content);
                    return Ok(StreamEvent::TextDelta {
                        index,
                        delta: content.to_owned(),
                    });
                }
                slot => {
                    return Err(ProviderError::InvalidResponse(format!(
                        "Cannot apply a text delta to {slot:?}"
                    )));
                }
            }
        }
        let index = self.parts.len();
        self.parts.push(PartSlot::Text(content.to_owned()));
        self.by_vendor.insert(vendor, index);
        Ok(StreamEvent::TextStart {
            index,
            initial: content.to_owned(),
        })
    }

    /// `handle_tool_call_delta` (`_parts_manager.py:292-372`) for the
    /// turn subset: create or update the vendor part's call slot.
    /// Name fragments append; string args append; dict args merge; a
    /// call id replaces; mixing string and dict args raises, like
    /// `ToolCallPartDelta.apply` (`messages.py:2600-2660`).
    fn tool_delta(
        &mut self,
        vendor: VendorId,
        tool_name: Option<&str>,
        args: Option<ToolArgsDelta>,
        tool_call_id: Option<&str>,
    ) -> Result<(), ProviderError> {
        let index = match self.by_vendor.get(&vendor) {
            Some(&index) => match &self.parts[index] {
                PartSlot::ToolDelta { .. } => index,
                slot => {
                    return Err(ProviderError::InvalidResponse(format!(
                        "Cannot apply a tool call delta to {slot:?}"
                    )));
                }
            },
            None => {
                let index = self.parts.len();
                self.parts.push(PartSlot::ToolDelta {
                    name: None,
                    args: None,
                    call_id: None,
                });
                self.by_vendor.insert(vendor, index);
                index
            }
        };
        let PartSlot::ToolDelta {
            name,
            args: slot_args,
            call_id,
        } = &mut self.parts[index]
        else {
            unreachable!("slot checked above");
        };
        if let Some(fragment) = tool_name {
            match name {
                Some(have) => have.push_str(fragment),
                None => *name = Some(fragment.to_owned()),
            }
        }
        match args {
            None => {}
            Some(ToolArgsDelta::Str(fragment)) => match slot_args {
                None => *slot_args = Some(ToolArgs::Raw(fragment)),
                Some(ToolArgs::Raw(have)) => have.push_str(&fragment),
                Some(ToolArgs::Dict(_)) => {
                    return Err(ProviderError::InvalidResponse(
                        "Cannot apply JSON deltas to non-JSON tool arguments".to_owned(),
                    ));
                }
            },
            Some(ToolArgsDelta::Dict(incoming)) => match slot_args {
                None => *slot_args = Some(ToolArgs::Dict(incoming)),
                Some(ToolArgs::Dict(have)) => have.extend(incoming),
                Some(ToolArgs::Raw(_)) => {
                    return Err(ProviderError::InvalidResponse(
                        "Cannot apply dict deltas to non-dict tool arguments".to_owned(),
                    ));
                }
            },
        }
        // `if self.tool_call_id:` (`messages.py:2656,2687`): only a
        // non-empty id replaces; the final `or` in `as_part` then
        // generates exactly like the SDK.
        if let Some(id) = tool_call_id.filter(|id| !id.is_empty()) {
            *call_id = Some(id.to_owned());
        }
        Ok(())
    }

    /// `get_parts` (`_parts_manager.py:99-105`): complete parts in
    /// creation order; tool slots without a name are still deltas
    /// and excluded. A nameless-but-otherwise-complete slot keeps no
    /// id: `as_part` generates the id only with the name
    /// (`messages.py:2580-2595`).
    fn parts(&self) -> Vec<StreamPart> {
        let mut out = Vec::new();
        for slot in &self.parts {
            match slot {
                PartSlot::Text(text) => out.push(StreamPart::Text(text.clone())),
                PartSlot::ToolDelta {
                    name: Some(name),
                    args,
                    call_id,
                } => out.push(StreamPart::ToolCall(StreamToolCall {
                    name: name.clone(),
                    args: args.clone(),
                    call_id: call_id.clone().unwrap_or_else(generate_tool_call_id),
                })),
                PartSlot::ToolDelta { name: None, .. } => {}
            }
        }
        out
    }
}

/// One tool-arguments delta: an appended JSON string or a merged dict
/// (`ToolCallPartDelta.args_delta`, `messages.py:2543-2575`).
#[derive(Debug, Clone)]
enum ToolArgsDelta {
    Str(String),
    Dict(Map<String, Value>),
}

// ---------------------------------------------------------------------------
// SSE framing
// ---------------------------------------------------------------------------

/// One SSE dispatch: the `event` name (default `message`) plus the
/// `data` lines joined with `\n` (the SDK `ServerSentEvent` shape
/// both stream readers consume).
#[derive(Debug, Clone, PartialEq, Eq)]
struct SseEvent {
    name: String,
    data: String,
}

/// Incremental SSE framer over a byte stream: `feed` buffers chunk
/// bytes, `drain` yields complete dispatches (blank-line
/// terminated). Handles `\r\n` endings and `:` comment lines; a
/// trailing unterminated buffer yields nothing until more bytes (or
/// `finish`, which drops it — a truncated tail carries no dispatch).
#[derive(Debug, Default)]
struct SseFramer {
    buf: Vec<u8>,
}

impl SseFramer {
    fn feed(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    fn drain(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();
        let mut consumed = 0;
        let mut cursor = 0;
        // Byte ranges of the current dispatch's lines (ranges, not
        // slices, so the consumed prefix can drain at the end).
        let mut lines: Vec<(usize, usize)> = Vec::new();
        while cursor < self.buf.len() {
            let line_start = cursor;
            let line_end = match self.buf[cursor..].iter().position(|&b| b == b'\n') {
                Some(relative) => cursor + relative,
                None => break,
            };
            let mut end = line_end;
            if end > line_start && self.buf[end - 1] == b'\r' {
                end -= 1;
            }
            cursor = line_end + 1;
            if end == line_start {
                let slices: Vec<&[u8]> = lines
                    .iter()
                    .map(|&(from, to)| &self.buf[from..to])
                    .collect();
                if let Some(event) = SseEvent::dispatch(&slices) {
                    events.push(event);
                }
                lines.clear();
                consumed = cursor;
            } else {
                lines.push((line_start, end));
            }
        }
        self.buf.drain(..consumed);
        events
    }
}

impl SseEvent {
    fn dispatch(lines: &[&[u8]]) -> Option<SseEvent> {
        let mut name = "message".to_owned();
        let mut data: Vec<String> = Vec::new();
        for line in lines {
            if line.first() == Some(&b':') {
                continue;
            }
            let (field, value) = match line.iter().position(|&b| b == b':') {
                Some(at) => (&line[..at], &line[at + 1..]),
                None => (*line, &[][..]),
            };
            let value = if value.first() == Some(&b' ') {
                &value[1..]
            } else {
                value
            };
            match field {
                b"event" => name = String::from_utf8_lossy(value).into_owned(),
                b"data" => data.push(String::from_utf8_lossy(value).into_owned()),
                _ => {}
            }
        }
        if data.is_empty() {
            return None;
        }
        Some(SseEvent {
            name,
            data: data.join("\n"),
        })
    }
}

// ---------------------------------------------------------------------------
// OpenAI streaming
// ---------------------------------------------------------------------------

/// Incremental OpenAI chat-completions stream parser
/// (`OpenAIStreamedResponse._get_event_iterator`,
/// `openai.py:3229-3296`): `feed` chunk bytes, collect the live
/// text [`StreamEvent`]s, `finish` the assembled [`StreamedTurn`]
/// (the `get()` outcome after exhaustion).
///
/// `provider_url` and `model` feed usage extraction (`_map_usage`,
/// `openai.py:4223-4262`); usage accumulates per chunk (`self._usage
/// += chunk_usage`).
pub struct OpenAIStream<'a> {
    extract: &'a ProviderTable,
    provider_url: String,
    framer: SseFramer,
    manager: PartsManager,
    usage: RequestUsage,
    finish: Option<Finish>,
    refused: bool,
    refusal_text: String,
    model_name: String,
    response_id: Option<String>,
    provider_details: Option<Map<String, Value>>,
    done: bool,
    seen_chunk: bool,
}

impl<'a> OpenAIStream<'a> {
    pub fn new(extract: &'a ProviderTable, provider_url: &str, model: &str) -> Self {
        OpenAIStream {
            extract,
            provider_url: provider_url.to_owned(),
            framer: SseFramer::default(),
            manager: PartsManager::default(),
            usage: RequestUsage::zeros(),
            finish: None,
            refused: false,
            refusal_text: String::new(),
            model_name: model.to_owned(),
            response_id: None,
            provider_details: None,
            done: false,
            seen_chunk: false,
        }
    }

    /// Feed raw stream bytes; returns the text events the complete
    /// dispatches produced, in order. Feeding after `[DONE]` is a
    /// no-op (the SDK breaks out of the read loop).
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<StreamEvent>, ProviderError> {
        if self.done {
            return Ok(Vec::new());
        }
        self.framer.feed(bytes);
        let mut out = Vec::new();
        for event in self.framer.drain() {
            // `if sse.data.startswith("[DONE]"): break`
            // (`openai/_streaming.py:63-64`).
            if event.data.starts_with("[DONE]") {
                self.done = true;
                break;
            }
            self.chunk(&event.data, &mut out)?;
        }
        Ok(out)
    }

    /// Assemble the final result (`get()`): parts, accumulated
    /// usage, finish, and the provider stamps. A stream with zero
    /// data dispatches raises exactly like the SDK's empty-stream
    /// peek (`openai.py:1189-1192`).
    pub fn finish(self) -> Result<StreamedTurn, ProviderError> {
        if !self.seen_chunk {
            return Err(empty_stream());
        }
        let mut details = self.provider_details;
        if !self.refusal_text.is_empty() {
            details
                .get_or_insert_with(Map::new)
                .insert("refusal".to_owned(), Value::String(self.refusal_text));
        }
        Ok(StreamedTurn {
            parts: self.manager.parts(),
            usage: self.usage,
            finish: self.finish,
            model_name: self.model_name,
            response_id: self.response_id,
            provider_details: details,
        })
    }

    fn chunk(&mut self, data: &str, out: &mut Vec<StreamEvent>) -> Result<(), ProviderError> {
        let value: Value = serde_json::from_str(data).map_err(|_| {
            ProviderError::InvalidResponse(
                "Invalid response from openai chat completions endpoint, expected JSON data"
                    .to_owned(),
            )
        })?;
        // A mid-stream `{"error": ...}` data payload raises `APIError`
        // (`openai/_streaming.py:87-97`), which `_map_api_errors`
        // does not catch — its message classifies with no status.
        if let Some(error) = value.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .unwrap_or("An error occurred during streaming");
            return Err(ProviderError::InvalidResponse(message.to_owned()));
        }
        // The first chunk's `created` stamp lands in provider details
        // when truthy (`_provider_timestamp`,
        // `openai.py:1198,3232-3234`): real providers always send it,
        // the oracle's `0` leaves the key unset. Set before any
        // `finish_reason` insert so the key order stays
        // `timestamp`-first, like the SDK merge.
        if !self.seen_chunk {
            self.seen_chunk = true;
            if let Some(created) = value
                .get("created")
                .and_then(Value::as_i64)
                .filter(|created| *created != 0)
            {
                self.provider_details.get_or_insert_with(Map::new).insert(
                    "timestamp".to_owned(),
                    Value::String(stream_created_timestamp(created)),
                );
            }
        }
        let chunk_usage = value
            .get("usage")
            .and_then(Value::as_object)
            .map(|usage| {
                self.extract.extract_openai(
                    &self.provider_url,
                    &self.model_name.clone(),
                    Some(usage),
                )
            })
            .unwrap_or_else(RequestUsage::zeros);
        self.usage = add_request_usage(&self.usage, &chunk_usage);
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            if !id.is_empty() {
                self.response_id = Some(id.to_owned());
            }
        }
        if let Some(model) = value.get("model").and_then(Value::as_str) {
            if !model.is_empty() {
                self.model_name = model.to_owned();
            }
        }
        // Empty on the final usage-only chunk; `None` from
        // loose-constructed malformed chunks (`openai.py:3252-255`).
        let Some(choice) = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            return Ok(());
        };
        let Some(delta) = choice.get("delta") else {
            return Ok(());
        };
        if delta.is_null() {
            return Ok(());
        }
        // Refusals (structured-output safety filter) set
        // `content_filter` and skip part mapping (`openai.py:3264-71`).
        if let Some(refusal) = delta.get("refusal").and_then(Value::as_str) {
            if !refusal.is_empty() {
                self.refused = true;
                self.finish = Some(Finish::ContentFilter);
                self.refusal_text.push_str(refusal);
                return Ok(());
            }
        }
        if let Some(raw) = choice.get("finish_reason").and_then(Value::as_str) {
            if !raw.is_empty() && !self.refused {
                self.finish = openai_finish_reason(raw);
            }
        }
        // Provider details merge per chunk (`openai.py:3276-80`):
        // logprobs never occur on the turn path (the
        // `openai_logprobs` setting is never sent), so only the raw
        // finish reason lands here.
        if let Some(raw) = choice.get("finish_reason").and_then(Value::as_str) {
            // `if raw_finish_reason := ...`: empty reads unset.
            if !raw.is_empty() {
                let mut details = self.provider_details.take().unwrap_or_default();
                if self.refused {
                    details.shift_remove("finish_reason");
                } else {
                    details.insert("finish_reason".to_owned(), Value::String(raw.to_owned()));
                }
                self.provider_details = Some(details);
            }
        }
        // Text (`_map_text_delta`, `openai.py:3337-55`): the whole
        // stream shares the `'content'` vendor id. Thinking tags are
        // a turn-subset cut (see the module docs): content rides as
        // text.
        if let Some(content) = delta.get("content").and_then(Value::as_str) {
            if !content.is_empty() {
                out.push(self.manager.text_delta(VendorId::Content, content)?);
            }
        }
        // Tool calls (`_map_tool_call_delta`, `openai.py:3357-75`):
        // vendor id is the chunk `index`.
        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                let Some(index) = call.get("index").and_then(Value::as_i64) else {
                    return Err(ProviderError::InvalidResponse(
                        "Invalid response from openai chat completions endpoint, tool call without index"
                            .to_owned(),
                    ));
                };
                let function = call.get("function");
                let tool_name = function.and_then(|f| f.get("name")).and_then(Value::as_str);
                let args = function
                    .and_then(|f| f.get("arguments"))
                    .and_then(Value::as_str)
                    .map(|fragment| ToolArgsDelta::Str(fragment.to_owned()));
                let tool_call_id = call.get("id").and_then(Value::as_str);
                self.manager
                    .tool_delta(VendorId::Index(index), tool_name, args, tool_call_id)?;
            }
        }
        Ok(())
    }
}

/// The empty-stream failure both lanes raise when the peeked
/// first item is missing (`openai.py:1189-1192`,
/// `anthropic.py:980-982`): `UnexpectedModelBehavior`, which
/// classifies `internal` on text alone.
fn empty_stream() -> ProviderError {
    ProviderError::InvalidResponse(
        "Streamed response ended without content or tool calls".to_owned(),
    )
}

/// `number_to_datetime` (`_utils.py:547-548`): epoch seconds rendered
/// UTC ISO-8601 with the `Z` suffix pydantic's JSON mode emits
/// (probed: `1760716800` dumps `"2025-10-17T16:00:00Z"`). Same shape
/// as the D-11 non-streaming port, whose helper is crate-private —
/// but unlike that path there is no now-backfill: a falsy
/// first-chunk `created` leaves the `timestamp` detail unset.
fn stream_created_timestamp(created: i64) -> String {
    chrono::DateTime::from_timestamp(created, 0)
        .map(|when| when.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

/// `_CHAT_FINISH_REASON_MAP` (`openai.py:254-262`): unknown keys read
/// `None` (`.get`), like Python.
fn openai_finish_reason(raw: &str) -> Option<Finish> {
    match raw {
        "stop" => Some(Finish::Stop),
        "length" => Some(Finish::Length),
        "tool_calls" | "function_call" => Some(Finish::ToolCall),
        "content_filter" => Some(Finish::ContentFilter),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Anthropic streaming
// ---------------------------------------------------------------------------

/// Incremental Anthropic messages stream parser
/// (`AnthropicStreamedResponse._get_event_iterator`,
/// `anthropic.py:2176-2300`): same feed/finish shape as
/// [`OpenAIStream`]. Usage is *replaced* per usage event with merged
/// details (`_map_usage`, `anthropic.py:2123-2162`); the model name
/// comes from the opening `message_start`.
pub struct AnthropicStream<'a> {
    extract: &'a ProviderTable,
    provider_url: String,
    framer: SseFramer,
    manager: PartsManager,
    usage: RequestUsage,
    usage_details: Map<String, Value>,
    finish: Option<Finish>,
    model_name: String,
    response_id: Option<String>,
    provider_details: Option<Map<String, Value>>,
    seen_start: bool,
    seen_event: bool,
}

impl<'a> AnthropicStream<'a> {
    pub fn new(extract: &'a ProviderTable, provider_url: &str, model: &str) -> Self {
        AnthropicStream {
            extract,
            provider_url: provider_url.to_owned(),
            framer: SseFramer::default(),
            manager: PartsManager::default(),
            usage: RequestUsage::zeros(),
            usage_details: Map::new(),
            finish: None,
            model_name: model.to_owned(),
            response_id: None,
            provider_details: None,
            seen_start: false,
            seen_event: false,
        }
    }

    /// Feed raw stream bytes; returns the text events the complete
    /// dispatches produced, in order.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<StreamEvent>, ProviderError> {
        self.framer.feed(bytes);
        let mut out = Vec::new();
        for event in self.framer.drain() {
            // `ping` events are heartbeats (`anthropic/_streaming.py`).
            if event.name == "ping" {
                continue;
            }
            // An `error` event raises through `_make_status_error`
            // with the 200 response's status
            // (`anthropic/_streaming.py:154-167`), which
            // `_map_api_errors` downgrades to a status-less
            // `ModelAPIError` (`anthropic.py:245-252`): the message
            // is the Python `str` of the parsed body (a dict, so
            // `py_repr`), or the raw data when it is not JSON.
            if event.name == "error" {
                let message = match serde_json::from_str::<Value>(&event.data) {
                    Ok(body) => py_repr(&body),
                    Err(_) => {
                        if event.data.is_empty() {
                            "Error code: 200".to_owned()
                        } else {
                            event.data.clone()
                        }
                    }
                };
                return Err(ProviderError::InvalidResponse(message));
            }
            self.event(&event, &mut out)?;
        }
        Ok(out)
    }

    /// Assemble the final result (`get()`). A stream with zero
    /// events raises exactly like the SDK's empty-stream peek
    /// (`anthropic.py:980-982`); heartbeats never count (the SDK
    /// reader skips `ping` before the peek).
    pub fn finish(self) -> Result<StreamedTurn, ProviderError> {
        if !self.seen_event {
            return Err(empty_stream());
        }
        Ok(StreamedTurn {
            parts: self.manager.parts(),
            usage: self.usage,
            finish: self.finish,
            model_name: self.model_name,
            response_id: self.response_id,
            provider_details: self.provider_details,
        })
    }

    fn event(&mut self, event: &SseEvent, out: &mut Vec<StreamEvent>) -> Result<(), ProviderError> {
        let value: Value = serde_json::from_str(&event.data).map_err(|_| {
            ProviderError::InvalidResponse(
                "Invalid response from anthropic messages endpoint, expected JSON data".to_owned(),
            )
        })?;
        self.seen_event = true;
        // `_process_streamed_response` asserts the first event is a
        // `message_start` (`anthropic.py:975-995`).
        if !self.seen_start {
            self.seen_start = true;
            if event.name != "message_start" {
                return Err(ProviderError::InvalidResponse(
                    "Invalid response from anthropic messages endpoint, expected message_start"
                        .to_owned(),
                ));
            }
        }
        match event.name.as_str() {
            "message_start" => {
                let Some(message) = value.get("message") else {
                    return Err(invalid_anthropic("message_start without message"));
                };
                if message.is_null() {
                    // Bedrock type-less chunks (`anthropic.py:2182-88`):
                    // usage resets to zeros, nothing else updates.
                    self.usage = RequestUsage::zeros();
                    return Ok(());
                }
                let message_obj = message
                    .as_object()
                    .ok_or_else(|| invalid_anthropic("message_start without message"))?;
                if let Some(usage) = message_obj.get("usage").and_then(Value::as_object) {
                    self.replace_usage(usage);
                } else {
                    // `usage` is a required SDK field; its absence
                    // fails validation upstream, like the errors below.
                    return Err(invalid_anthropic("message_start without usage"));
                }
                if let Some(id) = message_obj.get("id").and_then(Value::as_str) {
                    self.response_id = Some(id.to_owned());
                }
                if let Some(model) = message_obj.get("model").and_then(Value::as_str) {
                    self.model_name = model.to_owned();
                }
                if let Some(container) = message_obj
                    .get("container")
                    .and_then(Value::as_object)
                    .and_then(|c| c.get("id"))
                    .and_then(Value::as_str)
                {
                    self.details().insert(
                        "container_id".to_owned(),
                        Value::String(container.to_owned()),
                    );
                }
            }
            "content_block_start" => {
                let (index, block) = self.block_start(&value)?;
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                out.push(self.manager.text_delta(VendorId::Index(index), text)?);
                            }
                        }
                    }
                    Some("tool_use") => {
                        let tool_name = block.get("name").and_then(Value::as_str);
                        // `args=cast(...) or None`: an empty input
                        // object reads `None` (`anthropic.py:2217-25`).
                        let args = match block.get("input") {
                            Some(Value::Object(map)) if !map.is_empty() => {
                                Some(ToolArgsDelta::Dict(map.clone()))
                            }
                            _ => None,
                        };
                        let tool_call_id = block.get("id").and_then(Value::as_str);
                        self.manager.tool_delta(
                            VendorId::Index(index),
                            tool_name,
                            args,
                            tool_call_id,
                        )?;
                    }
                    // Any other block kind (thinking, server tools,
                    // web search, MCP, compaction) falls through the
                    // `elif` chain with no `else`: ignored
                    // (`anthropic.py:2200-2265`).
                    _ => {}
                }
            }
            "content_block_delta" => {
                let index = self.block_index(&value)?;
                let Some(delta) = value.get("delta") else {
                    return Err(invalid_anthropic("content_block_delta without delta"));
                };
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        if let Some(text) = delta.get("text").and_then(Value::as_str) {
                            out.push(self.manager.text_delta(VendorId::Index(index), text)?);
                        }
                    }
                    Some("input_json_delta") => {
                        if let Some(fragment) = delta.get("partial_json").and_then(Value::as_str) {
                            self.manager.tool_delta(
                                VendorId::Index(index),
                                None,
                                Some(ToolArgsDelta::Str(fragment.to_owned())),
                                None,
                            )?;
                        }
                    }
                    // Citations are an explicit `pass`; thinking,
                    // signature, and compaction deltas belong to
                    // configurations the turn never enables
                    // (turn-subset cut, see the module docs).
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(usage) = value.get("usage").and_then(Value::as_object) {
                    self.replace_usage(usage);
                } else {
                    return Err(invalid_anthropic("message_delta without usage"));
                }
                if let Some(delta) = value.get("delta").and_then(Value::as_object) {
                    if let Some(raw) = delta.get("stop_reason").and_then(Value::as_str) {
                        // `if raw_finish_reason := ...`: empty reads unset.
                        if !raw.is_empty() {
                            self.details()
                                .insert("finish_reason".to_owned(), Value::String(raw.to_owned()));
                            self.finish = anthropic_finish_reason(raw);
                        }
                    }
                    if let Some(stop) = delta.get("stop_details").and_then(Value::as_object) {
                        if let Some(explanation) = stop.get("explanation").and_then(Value::as_str) {
                            self.details().insert(
                                "refusal".to_owned(),
                                Value::String(explanation.to_owned()),
                            );
                        }
                        if let Some(category) = stop.get("category").and_then(Value::as_str) {
                            self.details().insert(
                                "refusal_category".to_owned(),
                                Value::String(category.to_owned()),
                            );
                        }
                    }
                    if let Some(container) = delta
                        .get("container")
                        .and_then(Value::as_object)
                        .and_then(|c| c.get("id"))
                        .and_then(Value::as_str)
                    {
                        self.details().insert(
                            "container_id".to_owned(),
                            Value::String(container.to_owned()),
                        );
                    }
                }
            }
            // Block/stop markers only finalize server and MCP tool
            // state, unreachable on the turn path
            // (`anthropic.py:2272-2300`): no-ops.
            "content_block_stop" | "message_stop" => {}
            // Unknown event names fall through the `isinstance`
            // chain: ignored, like Python.
            _ => {}
        }
        Ok(())
    }

    /// Usage replacement (`_map_usage`, `anthropic.py:2123-2162`):
    /// the event's flat usage keys merge over the running details,
    /// then extraction runs over the merged map (compaction
    /// iteration keys only exist on streamed compaction deltas,
    /// unreachable on the turn path).
    fn replace_usage(&mut self, usage: &Map<String, Value>) {
        for key in [
            "input_tokens",
            "output_tokens",
            "cache_creation_input_tokens",
            "cache_read_input_tokens",
        ] {
            if let Some(Value::Number(number)) = usage.get(key) {
                self.usage_details
                    .insert(key.to_owned(), Value::Number(number.clone()));
            }
        }
        self.usage = self
            .extract
            .extract_anthropic(&self.provider_url, &self.usage_details);
    }

    fn details(&mut self) -> &mut Map<String, Value> {
        self.provider_details.get_or_insert_with(Map::new)
    }

    fn block_index(&self, value: &Value) -> Result<i64, ProviderError> {
        value
            .get("index")
            .and_then(Value::as_i64)
            .ok_or_else(|| invalid_anthropic("content block without index"))
    }

    fn block_start<'b>(&self, value: &'b Value) -> Result<(i64, &'b Value), ProviderError> {
        let index = self.block_index(value)?;
        let block = value
            .get("content_block")
            .filter(|block| !block.is_null())
            .ok_or_else(|| invalid_anthropic("content_block_start without content_block"))?;
        Ok((index, block))
    }
}

fn invalid_anthropic(detail: &str) -> ProviderError {
    ProviderError::InvalidResponse(format!(
        "Invalid response from anthropic messages endpoint, {detail}"
    ))
}

/// `_FINISH_REASON_MAP` (`anthropic.py:87-96`): unknown keys read
/// `None` (`.get`), like Python.
fn anthropic_finish_reason(raw: &str) -> Option<Finish> {
    match raw {
        "compaction" | "end_turn" | "stop_sequence" | "pause_turn" => Some(Finish::Stop),
        "max_tokens" | "model_context_window_exceeded" => Some(Finish::Length),
        "tool_use" => Some(Finish::ToolCall),
        "refusal" => Some(Finish::ContentFilter),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Usage accounting
// ---------------------------------------------------------------------------

/// Add two per-response usages (`RequestUsage.__add__` /
/// `_incr_usage_tokens`, `usage.py:136-145,240-255`): the six
/// counters plus the numeric `details` merge. `output_audio_tokens`
/// is deliberately NOT accumulated (the function never touches it —
/// the same quirk the D-11 `RunUsage::incr` ports). Bools count as
/// ints; a non-numeric incumbent plus numeric incoming keeps its
/// value (unreachable from conforming providers).
pub fn add_request_usage(left: &RequestUsage, right: &RequestUsage) -> RequestUsage {
    let mut merged = RequestUsage {
        input_tokens: left.input_tokens.add(right.input_tokens),
        cache_write_tokens: left.cache_write_tokens.add(right.cache_write_tokens),
        cache_read_tokens: left.cache_read_tokens.add(right.cache_read_tokens),
        output_tokens: left.output_tokens.add(right.output_tokens),
        input_audio_tokens: left.input_audio_tokens.add(right.input_audio_tokens),
        cache_audio_read_tokens: left
            .cache_audio_read_tokens
            .add(right.cache_audio_read_tokens),
        output_audio_tokens: left.output_audio_tokens,
        details: left.details.clone(),
    };
    for (key, value) in &right.details {
        let incoming = match value {
            Value::Number(number) => number.clone(),
            Value::Bool(flag) => i64::from(*flag).into(),
            _ => continue,
        };
        let have = match merged.details.remove(key) {
            Some(Value::Number(have)) => have,
            Some(Value::Bool(have)) => i64::from(have).into(),
            Some(kept) => {
                merged.details.insert(key.clone(), kept);
                continue;
            }
            None => 0.into(),
        };
        merged.details.insert(
            key.clone(),
            add_json_numbers(&have, &incoming).unwrap_or(Value::Null),
        );
    }
    merged
}

/// Add two JSON numbers the way the `details` merge does: int+int
/// stays int (saturating), anything float-involved becomes a float.
fn add_json_numbers(left: &serde_json::Number, right: &serde_json::Number) -> Option<Value> {
    match (left.as_i64(), right.as_i64()) {
        (Some(left), Some(right)) => Some(Value::Number(left.saturating_add(right).into())),
        _ => {
            let sum = left.as_f64()? + right.as_f64()?;
            serde_json::Number::from_f64(sum).map(Value::Number)
        }
    }
}

// ---------------------------------------------------------------------------
// Streaming HTTP client
// ---------------------------------------------------------------------------

/// One streaming provider request: the lane (headers, auth, model),
/// the endpoint URL ([`stream_endpoint`], or a test stub), the
/// encoded body, the per-request timeout, and the usage extractor.
pub struct StreamRequest<'a> {
    pub lane: &'a Lane,
    pub endpoint_url: String,
    pub body: Vec<u8>,
    /// Per-request timeout, seconds (the transport-level `timeout`,
    /// never a body key). Zero clamps to one second — a zero
    /// `Duration` would time out immediately — so pass the SDK
    /// default (600) explicitly for default behavior.
    pub timeout_secs: u64,
    pub extract: &'a ProviderTable,
}

/// POST a streaming turn and assemble the result (`request_stream`,
/// `openai.py:935-951` / `anthropic.py:560-580`): the stainless
/// retry loop around the initial request, then chunk reads through
/// the lane parser. Live text events stream to `on_event` as they
/// arrive; the returned [`StreamedTurn`] is the exhausted `get()`.
///
/// Reads via `Response::chunk` (no `stream` feature needed).
/// Mid-stream transport failures read as timeout/connection errors,
/// like the SDK readers letting bare httpx errors propagate into
/// `_map_api_errors`.
pub async fn stream_turn(
    client: &reqwest::Client,
    request: &StreamRequest<'_>,
    mut on_event: impl FnMut(StreamEvent),
) -> Result<StreamedTurn, ProviderError> {
    let mut response = post_stream(request, client).await?;
    let provider_url = request.lane.provider_url();
    let model = request.lane.model_name().to_owned();
    match request.lane {
        Lane::OpenAICompatible { .. } => {
            let mut stream = OpenAIStream::new(request.extract, &provider_url, &model);
            loop {
                match response.chunk().await {
                    Ok(Some(bytes)) => {
                        for event in stream.feed(&bytes)? {
                            on_event(event);
                        }
                    }
                    Ok(None) => break,
                    Err(error) if error.is_timeout() => return Err(ProviderError::Timeout),
                    Err(_) => return Err(ProviderError::Connection),
                }
            }
            Ok(stream.finish()?)
        }
        Lane::Anthropic { .. } => {
            let mut stream = AnthropicStream::new(request.extract, &provider_url, &model);
            loop {
                match response.chunk().await {
                    Ok(Some(bytes)) => {
                        for event in stream.feed(&bytes)? {
                            on_event(event);
                        }
                    }
                    Ok(None) => break,
                    Err(error) if error.is_timeout() => return Err(ProviderError::Timeout),
                    Err(_) => return Err(ProviderError::Connection),
                }
            }
            Ok(stream.finish()?)
        }
    }
}

/// POST with the stainless retry loop
/// (`openai/_base_client.py:990-1060`, async mirror at 1610-1660;
/// identical in the Anthropic SDK): timeouts and connection errors
/// retry while attempts remain, else surface; error statuses retry
/// per `_should_retry`, else surface with the parsed-or-text body.
/// The 200 response returns open for streaming (only the status and
/// headers are read here — the body streams through the parser).
async fn post_stream(
    request: &StreamRequest<'_>,
    client: &reqwest::Client,
) -> Result<reqwest::Response, ProviderError> {
    let mut retries_taken: u32 = 0;
    loop {
        let mut outgoing = client
            .post(request.endpoint_url.clone())
            .header("Content-Type", "application/json")
            .header("User-Agent", USER_AGENT)
            .timeout(std::time::Duration::from_secs(request.timeout_secs.max(1)));
        match request.lane {
            Lane::Anthropic { api_key, .. } => {
                outgoing = outgoing
                    .header("x-api-key", api_key.as_str())
                    .header("anthropic-version", ANTHROPIC_VERSION);
            }
            Lane::OpenAICompatible { api_key, .. } => {
                outgoing = outgoing.bearer_auth(api_key);
            }
        }
        // The SDKs' `X-Stainless-*` telemetry headers identify the
        // Python HTTP stack, not the request: deliberately not sent
        // (same call as the D-11 lanes).
        match outgoing.body(request.body.clone()).send().await {
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
                if status < 400 {
                    return Ok(response);
                }
                if retries_taken < MAX_RETRIES && should_retry_status(status, response.headers()) {
                    sleep_for_retry(retries_taken, Some(response.headers())).await;
                    retries_taken += 1;
                    continue;
                }
                let bytes = response.bytes().await.unwrap_or_default();
                let body_text = String::from_utf8_lossy(&bytes).into_owned();
                let body_json = serde_json::from_slice::<Value>(&bytes).ok();
                return Err(ProviderError::Http {
                    status,
                    body_text,
                    body_json,
                });
            }
        }
    }
}

// The stainless retry contract (`openai/_constants.py:10-14`) lives in
// the canonical [`pidash_services::assistant::retry`] policy (OVL-07
// moved the former private copy there; the D-11 send loop shares it).

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

/// Word a streaming provider failure the way the drive's
/// `turn_error_for_user` hook does: the [`ProviderError`] message (the
/// `ModelHTTPError` / `ModelAPIError` text), the HTTP status and the
/// structured-body one-liner when the failure carries them, and the
/// lane's model name, through
/// [`classify_provider_error`][pidash_jobs::assistant::classify_provider_error]
/// with the BYOK connection (`tasks.py` `_classify_error:447-486`).
/// Only `Http` carries a `status_code` attribute and a body —
/// timeouts, connections, mid-stream errors, and malformed payloads
/// classify on text alone. `None` is the generic `internal` bucket.
#[allow(dead_code)] // the loop hands the `TurnLoopError` to the drive's seam hook; pinned here by tests
pub fn classify_provider_error(lane: &Lane, error: &ProviderError) -> Option<(String, String)> {
    use pidash_jobs::assistant::{classify_provider_error, provider_detail, BYOK};
    let (status, body_detail) = match error {
        ProviderError::Http {
            status, body_json, ..
        } => (Some(*status), provider_detail(body_json.as_ref())),
        ProviderError::Timeout | ProviderError::Connection | ProviderError::InvalidResponse(_) => {
            (None, String::new())
        }
    };
    classify_provider_error(
        status,
        &error.message(lane),
        &body_detail,
        BYOK,
        lane.model_name(),
    )
}

#[cfg(test)]
mod tests {
    use super::super::history::{
        parse_blob, TurnOutcome, TurnRequestPart, TurnResponsePart, TurnState,
    };
    use super::*;
    use pidash_jobs::oss_worker::model_runtime::schemas::Golden;
    use pidash_jobs::oss_worker::model_runtime::usage::UsageNumber;
    use std::path::PathBuf;

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/assistant/turn-runtime")
    }

    fn load_fixture(name: &str) -> Value {
        let path = fixture_dir().join(format!("{name}.json"));
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("fixture present: {name}"));
        serde_json::from_str(&text).expect("fixture parses")
    }

    fn provider_table() -> ProviderTable {
        let golden = Golden::load().expect("golden parses");
        ProviderTable::load(&golden).expect("provider table loads")
    }

    /// Wire tools from a captured request body, including the
    /// inferred `strict` flag where the capture carries it (some
    /// OpenAI-lane tools do; the Anthropic lane never does).
    fn wire_tools_from_body(body: &Value) -> Vec<WireTool> {
        body.get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|tool| {
                let (name, description, parameters, strict) = if tool.get("function").is_some() {
                    let function = tool.get("function").unwrap();
                    (
                        function.get("name").and_then(Value::as_str).unwrap(),
                        function.get("description").and_then(Value::as_str).unwrap(),
                        function.get("parameters").cloned().unwrap(),
                        function.get("strict").and_then(Value::as_bool),
                    )
                } else {
                    assert!(
                        tool.get("strict").is_none(),
                        "Anthropic captures carry no strict key: {tool}"
                    );
                    (
                        tool.get("name").and_then(Value::as_str).unwrap(),
                        tool.get("description").and_then(Value::as_str).unwrap(),
                        tool.get("input_schema").cloned().unwrap(),
                        None,
                    )
                };
                WireTool {
                    name: name.to_owned(),
                    description: description.to_owned(),
                    parameters,
                    strict,
                }
            })
            .collect()
    }

    /// Instruction parts from a captured OpenAI body: the leading
    /// system-role messages (exactly the static literal plus the
    /// dynamic line on every capture).
    fn openai_instructions_from_body(body: &Value) -> Vec<String> {
        let messages = body.get("messages").and_then(Value::as_array).unwrap();
        let systems: Vec<String> = messages
            .iter()
            .take_while(|m| m.get("role").and_then(Value::as_str) == Some("system"))
            .map(|m| m.get("content").and_then(Value::as_str).unwrap().to_owned())
            .collect();
        assert!(
            messages
                .iter()
                .skip(systems.len())
                .all(|m| m.get("role").and_then(Value::as_str) != Some("system")),
            "no system message past the leading run"
        );
        assert_eq!(systems.len(), 2, "static + dynamic instructions");
        systems
    }

    /// Instruction parts from a captured Anthropic body: the system
    /// text blocks.
    fn anthropic_instructions_from_body(body: &Value) -> Vec<String> {
        body.get("system")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|block| {
                assert_eq!(
                    block.get("type").and_then(Value::as_str),
                    Some("text"),
                    "system blocks are text"
                );
                block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }

    /// Stored `model_messages` blobs paired with their exchanges:
    /// single-turn fixtures carry one blob, the replay fixture one
    /// blob per turn, failure fixtures none.
    fn stored_histories(fixture: &Value) -> Vec<Vec<TurnMessage>> {
        if let Some(turns) = fixture.get("turns").and_then(Value::as_array) {
            return turns
                .iter()
                .map(|turn| parse_blob(turn.get("model_messages").unwrap()).unwrap())
                .collect();
        }
        match fixture.get("model_messages") {
            None | Some(Value::Null) => Vec::new(),
            Some(blob) => vec![parse_blob(blob).unwrap()],
        }
    }

    /// First differing byte between two bodies, for failure output
    /// (the byte-evidence rule: lengths plus offset and hex).
    fn byte_diff(want: &[u8], got: &[u8]) -> String {
        let at = want
            .iter()
            .zip(got.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(want.len().min(got.len()));
        let window = |bytes: &[u8]| {
            let from = at.saturating_sub(40);
            let to = (at + 80).min(bytes.len());
            String::from_utf8_lossy(&bytes[from..to]).into_owned()
        };
        format!(
            "lengths want={} got={} first_diff_at={} want_hex={} got_hex={}\nwant_ctx={:?}\ngot_ctx={:?}",
            want.len(),
            got.len(),
            at,
            want.get(at).map(|b| format!("{b:02x}")).unwrap_or("-".to_owned()),
            got.get(at).map(|b| format!("{b:02x}")).unwrap_or("-".to_owned()),
            window(want),
            window(got),
        )
    }

    /// Assert captured request bytes against a built body:
    /// byte-identical everywhere except tool-return renderings,
    /// which compare at the JSON-value level (stored blobs are
    /// Postgres-jsonb-ordered, so a tool result's original key order
    /// is unrecoverable from the blob alone). Everything else — key
    /// order, separators, Unicode, rendered retry JSON, verbatim
    /// argument strings — compares byte for byte. `history` walks in
    /// lockstep with both bodies to find the substituted positions.
    /// Exchanges without tool returns compare fully byte-identical.
    fn assert_request_bytes(
        name: &str,
        index: usize,
        want_text: &str,
        built: &Value,
        history: &[TurnMessage],
        lane: &str,
    ) {
        // Parsing preserves document order: the want body keeps the
        // original wire key order.
        let mut want: Value = serde_json::from_str(want_text).unwrap();
        // The encoder round-trips the SDK's own bytes for this
        // document (separators, escapes, Unicode, key order).
        assert!(
            encode_body(&want).as_slice() == want_text.as_bytes(),
            "{name}#{index} encoder reproduces captured bytes:\n{}",
            byte_diff(want_text.as_bytes(), &encode_body(&want))
        );
        let mut got = built.clone();
        let pairs = substitute_tool_contents(&mut want, &mut got, history, lane, name, index);
        let want_bytes = encode_body(&want);
        let got_bytes = encode_body(&got);
        assert!(
            want_bytes == got_bytes,
            "{name}#{index} request bytes identical outside tool contents:\n{}",
            byte_diff(&want_bytes, &got_bytes)
        );
        for (at, (want_content, got_content)) in pairs.iter().enumerate() {
            assert_eq!(
                normalize_json_strings(want_content),
                normalize_json_strings(got_content),
                "{name}#{index} tool content {at} value-equal"
            );
        }
    }

    fn exchange_body_bytes(exchange: &Value, name: &str, index: usize) -> String {
        exchange
            .get("request")
            .and_then(|r| r.get("body_bytes"))
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{name}#{index} captures body_bytes"))
            .to_owned()
    }

    /// Normalize a rendered tool content for order-insensitive
    /// comparison: strings that parse as JSON compare as values
    /// (object key order ignored), anything else verbatim —
    /// recursing through arrays and objects (Anthropic block
    /// arrays).
    fn normalize_json_strings(value: &Value) -> Value {
        match value {
            Value::String(text) => serde_json::from_str(text).unwrap_or_else(|_| value.clone()),
            Value::Array(items) => Value::Array(items.iter().map(normalize_json_strings).collect()),
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(key, item)| (key.clone(), normalize_json_strings(item)))
                    .collect(),
            ),
            _ => value.clone(),
        }
    }

    /// Replace every tool-return rendering in both bodies with `""`
    /// and return the original `(want, got)` content pairs, walking
    /// `history` in lockstep with the wire messages. Retry feedback
    /// stays byte-compared: it is deterministically rendered, never
    /// re-serialized storage.
    fn substitute_tool_contents(
        want: &mut Value,
        got: &mut Value,
        history: &[TurnMessage],
        lane: &str,
        name: &str,
        index: usize,
    ) -> Vec<(Value, Value)> {
        let mut pairs = Vec::new();
        if lane == "openai" {
            let want_messages = want
                .get_mut("messages")
                .and_then(Value::as_array_mut)
                .unwrap();
            let got_messages = got
                .get_mut("messages")
                .and_then(Value::as_array_mut)
                .unwrap();
            assert_eq!(
                want_messages.len(),
                got_messages.len(),
                "{name}#{index} message counts"
            );
            let systems = want_messages
                .iter()
                .take_while(|m| m.get("role").and_then(Value::as_str) == Some("system"))
                .count();
            let mut cursor = systems;
            for message in history {
                match message {
                    TurnMessage::Request { parts, .. } => {
                        for part in parts {
                            if matches!(part, TurnRequestPart::ToolReturn { .. }) {
                                let want_content =
                                    want_messages[cursor].get_mut("content").unwrap();
                                let got_content = got_messages[cursor].get_mut("content").unwrap();
                                pairs.push((
                                    want_content.clone(),
                                    std::mem::replace(got_content, Value::String(String::new())),
                                ));
                                *want_content = Value::String(String::new());
                            }
                            cursor += 1;
                        }
                    }
                    TurnMessage::Response { parts, .. } => {
                        if !parts.is_empty() {
                            cursor += 1;
                        }
                    }
                }
            }
            assert_eq!(
                cursor,
                want_messages.len(),
                "{name}#{index} walk covers all messages"
            );
        } else {
            let want_messages = want
                .get_mut("messages")
                .and_then(Value::as_array_mut)
                .unwrap();
            let got_messages = got
                .get_mut("messages")
                .and_then(Value::as_array_mut)
                .unwrap();
            assert_eq!(
                want_messages.len(),
                got_messages.len(),
                "{name}#{index} message counts"
            );
            let mut cursor = 0;
            for message in history {
                match message {
                    TurnMessage::Request { parts, .. } => {
                        let mut block = 0;
                        let mut emitted = false;
                        for part in parts {
                            match part {
                                TurnRequestPart::User { text, .. } => {
                                    if !text.is_empty() {
                                        block += 1;
                                        emitted = true;
                                    }
                                }
                                TurnRequestPart::ToolReturn { .. } => {
                                    let want_block = want_messages[cursor]
                                        .get_mut("content")
                                        .and_then(Value::as_array_mut)
                                        .unwrap();
                                    let got_block = got_messages[cursor]
                                        .get_mut("content")
                                        .and_then(Value::as_array_mut)
                                        .unwrap();
                                    let want_content =
                                        want_block[block].get_mut("content").unwrap();
                                    let got_content = got_block[block].get_mut("content").unwrap();
                                    pairs.push((
                                        want_content.clone(),
                                        std::mem::replace(
                                            got_content,
                                            Value::String(String::new()),
                                        ),
                                    ));
                                    *want_content = Value::String(String::new());
                                    block += 1;
                                    emitted = true;
                                }
                                TurnRequestPart::Retry { .. } => {
                                    block += 1;
                                    emitted = true;
                                }
                            }
                        }
                        if emitted {
                            cursor += 1;
                        }
                    }
                    TurnMessage::Response { parts, .. } => {
                        // Like the builder: a block is emitted for every
                        // tool call and every non-empty text, and the
                        // message only when a block exists.
                        let emitted = parts.iter().any(|part| match part {
                            TurnResponsePart::ToolCall { .. } => true,
                            TurnResponsePart::Text { text, .. } => !text.is_empty(),
                        });
                        if emitted {
                            cursor += 1;
                        }
                    }
                }
            }
            assert_eq!(
                cursor,
                want_messages.len(),
                "{name}#{index} walk covers all messages"
            );
        }
        pairs
    }

    /// Every OpenAI-lane exchange whose history Django stored
    /// rebuilds to the captured request bytes exactly.
    #[test]
    fn openai_request_bytes_match_captures() {
        for name in [
            "text_openai",
            "tool_local",
            "tool_mcp",
            "retry_validation",
            "replay_3turn",
        ] {
            let fixture = load_fixture(name);
            let exchanges = fixture
                .get("provider_exchanges")
                .and_then(Value::as_array)
                .unwrap();
            let histories = stored_histories(&fixture);
            if name == "replay_3turn" {
                assert_eq!(histories.len(), 3, "one blob per turn");
                for (turn, blob) in histories.iter().enumerate() {
                    assert_eq!(
                        blob.len(),
                        2,
                        "{name} turn {turn} is one request + response"
                    );
                    assert!(
                        matches!(blob[0], TurnMessage::Request { .. })
                            && matches!(blob[1], TurnMessage::Response { .. }),
                        "{name} turn {turn} shape"
                    );
                }
                for (index, exchange) in exchanges.iter().enumerate() {
                    let mut history: Vec<TurnMessage> = histories
                        .iter()
                        .take(index)
                        .flat_map(|blob| blob.iter().cloned())
                        .collect();
                    history.push(histories[index][0].clone());
                    let body = exchange.get("request").and_then(|r| r.get("body")).unwrap();
                    let built = openai_request_body(
                        body.get("model").and_then(Value::as_str).unwrap(),
                        &openai_instructions_from_body(body),
                        &history,
                        &wire_tools_from_body(body),
                    );
                    assert_request_bytes(
                        name,
                        index,
                        &exchange_body_bytes(exchange, name, index),
                        &built,
                        &history,
                        "openai",
                    );
                }
                continue;
            }
            assert_eq!(histories.len(), 1, "{name} carries one blob");
            let messages = &histories[0];
            assert_eq!(
                messages.len(),
                exchanges.len() * 2,
                "{name} blob holds one request+response per exchange"
            );
            for (index, exchange) in exchanges.iter().enumerate() {
                let history = &messages[..index * 2 + 1];
                let body = exchange.get("request").and_then(|r| r.get("body")).unwrap();
                let built = openai_request_body(
                    body.get("model").and_then(Value::as_str).unwrap(),
                    &openai_instructions_from_body(body),
                    history,
                    &wire_tools_from_body(body),
                );
                assert_request_bytes(
                    name,
                    index,
                    &exchange_body_bytes(exchange, name, index),
                    &built,
                    history,
                    "openai",
                );
            }
        }
    }

    /// Every Anthropic-lane exchange rebuilds to the captured
    /// request bytes exactly.
    #[test]
    fn anthropic_request_bytes_match_captures() {
        for name in ["text_anthropic", "tool_anthropic"] {
            let fixture = load_fixture(name);
            let exchanges = fixture
                .get("provider_exchanges")
                .and_then(Value::as_array)
                .unwrap();
            let histories = stored_histories(&fixture);
            assert_eq!(histories.len(), 1, "{name} carries one blob");
            let messages = &histories[0];
            assert_eq!(
                messages.len(),
                exchanges.len() * 2,
                "{name} blob holds one request+response per exchange"
            );
            for (index, exchange) in exchanges.iter().enumerate() {
                let history = &messages[..index * 2 + 1];
                let body = exchange.get("request").and_then(|r| r.get("body")).unwrap();
                let instructions = anthropic_instructions_from_body(body);
                assert_eq!(instructions.len(), 2, "static + dynamic instructions");
                let built = anthropic_request_body(
                    body.get("model").and_then(Value::as_str).unwrap(),
                    &instructions,
                    history,
                    &wire_tools_from_body(body),
                );
                assert_request_bytes(
                    name,
                    index,
                    &exchange_body_bytes(exchange, name, index),
                    &built,
                    history,
                    "anthropic",
                );
            }
        }
    }

    /// The failure fixtures store no blob (the turn failed before
    /// `dump_new_messages`), so the single-exchange history is the
    /// user message row the turn ran on; the request (systems,
    /// tools, flags, key order) still rebuilds byte-identical.
    #[test]
    fn error_request_bytes_match_captures() {
        for name in ["error_401", "error_402"] {
            let fixture = load_fixture(name);
            let exchanges = fixture
                .get("provider_exchanges")
                .and_then(Value::as_array)
                .unwrap();
            assert_eq!(exchanges.len(), 1, "{name} fails on the first request");
            let user_text = fixture
                .get("messages")
                .and_then(Value::as_array)
                .unwrap()
                .iter()
                .find(|m| m.get("kind").and_then(Value::as_str) == Some("user"))
                .and_then(|m| m.get("display_content"))
                .and_then(Value::as_str)
                .unwrap();
            let history = vec![TurnMessage::Request {
                parts: vec![TurnRequestPart::User {
                    text: user_text.to_owned(),
                    timestamp: None,
                }],
                timestamp: None,
                instructions: None,
                run_id: None,
                conversation_id: None,
                metadata: Value::Null,
            }];
            let body = exchanges[0]
                .get("request")
                .and_then(|r| r.get("body"))
                .unwrap();
            let built = openai_request_body(
                body.get("model").and_then(Value::as_str).unwrap(),
                &openai_instructions_from_body(body),
                &history,
                &wire_tools_from_body(body),
            );
            assert_request_bytes(
                name,
                0,
                &exchange_body_bytes(&exchanges[0], name, 0),
                &built,
                &history,
                "openai",
            );
        }
    }

    /// The usage-limit turn stores no blob (it failed before
    /// `dump_new_messages`), so its 21 exchanges replay chained:
    /// each request builds from the previous responses parsed out
    /// of their SSE plus tool returns. Response parts come from the
    /// parsers under test; the tool result (an empty rendering the
    /// oracle produced) enters as its pre-render value.
    #[test]
    fn usage_limit_request_bytes_chain() {
        let name = "usage_limit";
        let fixture = load_fixture(name);
        let exchanges = fixture
            .get("provider_exchanges")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(exchanges.len(), 21, "one request per tool call");
        let table = provider_table();
        let first_body = exchanges[0]
            .get("request")
            .and_then(|r| r.get("body"))
            .unwrap();
        let user_text = first_body
            .get("messages")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .find(|m| m.get("role").and_then(Value::as_str) == Some("user"))
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str)
            .unwrap()
            .to_owned();
        let mut history = vec![TurnMessage::Request {
            parts: vec![TurnRequestPart::User {
                text: user_text,
                timestamp: None,
            }],
            timestamp: None,
            instructions: None,
            run_id: None,
            conversation_id: None,
            metadata: Value::Null,
        }];
        for (index, exchange) in exchanges.iter().enumerate() {
            let body = exchange.get("request").and_then(|r| r.get("body")).unwrap();
            let built = openai_request_body(
                body.get("model").and_then(Value::as_str).unwrap(),
                &openai_instructions_from_body(body),
                &history,
                &wire_tools_from_body(body),
            );
            assert_request_bytes(
                name,
                index,
                &exchange_body_bytes(exchange, name, index),
                &built,
                &history,
                "openai",
            );
            // Extend the chain from this exchange's own stream: the
            // parser's parts become the response, the tool result the
            // next request.
            let sse = exchange
                .get("response")
                .and_then(|r| r.get("sse_text"))
                .and_then(Value::as_str)
                .unwrap();
            let mut stream = OpenAIStream::new(&table, "http://stub/v1/", "oracle-stub");
            let events = stream.feed(sse.as_bytes()).unwrap();
            assert!(
                events.is_empty(),
                "{name}#{index} tool-only stream emits no text events"
            );
            let turned = stream.finish().unwrap();
            let mut response_parts = Vec::new();
            let mut returns = Vec::new();
            for part in &turned.parts {
                let StreamPart::ToolCall(call) = part else {
                    panic!("{name}#{index} streams tool calls only");
                };
                response_parts.push(TurnResponsePart::ToolCall {
                    name: call.name.clone(),
                    args: call.args.clone(),
                    call_id: call.call_id.clone(),
                    id: None,
                    provider_name: None,
                    provider_details: None,
                });
                returns.push(TurnRequestPart::ToolReturn {
                    name: call.name.clone(),
                    content: Value::String(String::new()),
                    call_id: call.call_id.clone(),
                    timestamp: None,
                    metadata: Value::Null,
                    outcome: TurnOutcome::Success,
                });
            }
            assert_eq!(
                response_parts.len(),
                1,
                "{name}#{index} one call per stream"
            );
            history.push(TurnMessage::Response {
                parts: response_parts,
                usage: RequestUsage::zeros(),
                model_name: None,
                timestamp: String::new(),
                provider_name: None,
                provider_url: None,
                provider_details: None,
                response_id: None,
                finish: None,
                run_id: None,
                conversation_id: None,
                metadata: Value::Null,
                state: TurnState::Complete,
            });
            history.push(TurnMessage::Request {
                parts: returns,
                timestamp: None,
                instructions: None,
                run_id: None,
                conversation_id: None,
                metadata: Value::Null,
            });
        }
    }

    /// `o1-mini` rejects the `system` role, so its instructions
    /// travel as `user` messages after the leading user run
    /// (`profiles/openai.py:218`); every other model inserts
    /// `system` messages before the first non-system message.
    #[test]
    fn instruction_roles_and_positions() {
        let history = vec![TurnMessage::Request {
            parts: vec![TurnRequestPart::User {
                text: "hi".to_owned(),
                timestamp: None,
            }],
            timestamp: None,
            instructions: None,
            run_id: None,
            conversation_id: None,
            metadata: Value::Null,
        }];
        let instructions = vec!["base".to_owned(), "dynamic".to_owned()];
        let body = openai_request_body("o1-mini-2024", &instructions, &history, &[]);
        let messages = body.get("messages").and_then(Value::as_array).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(
            messages[0].get("role").and_then(Value::as_str),
            Some("user")
        );
        assert_eq!(
            messages[0].get("content").and_then(Value::as_str),
            Some("hi")
        );
        assert_eq!(
            messages[1].get("role").and_then(Value::as_str),
            Some("user")
        );
        assert_eq!(
            messages[1].get("content").and_then(Value::as_str),
            Some("base")
        );
        assert_eq!(
            messages[2].get("content").and_then(Value::as_str),
            Some("dynamic")
        );
        let body = openai_request_body("oracle-stub", &instructions, &history, &[]);
        let messages = body.get("messages").and_then(Value::as_array).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(
            messages[0].get("role").and_then(Value::as_str),
            Some("system")
        );
        assert_eq!(
            messages[2].get("role").and_then(Value::as_str),
            Some("user")
        );
        // Empty tools omit the key (`tools or OMIT`).
        assert!(body.get("tools").is_none());
        let body = anthropic_request_body("claude-stub", &[], &history, &[]);
        assert!(body.get("system").is_none());
        assert!(body.get("tools").is_none());
    }

    /// Stored usage dicts (ints on every capture) as `RequestUsage`.
    fn request_usage_from_stored(usage: &Value) -> RequestUsage {
        let int = |key: &str| {
            usage
                .get(key)
                .and_then(Value::as_i64)
                .unwrap_or_else(|| panic!("stored usage carries {key}"))
        };
        RequestUsage {
            input_tokens: UsageNumber::Int(int("input_tokens")),
            cache_write_tokens: UsageNumber::Int(int("cache_write_tokens")),
            cache_read_tokens: UsageNumber::Int(int("cache_read_tokens")),
            output_tokens: UsageNumber::Int(int("output_tokens")),
            input_audio_tokens: UsageNumber::Int(int("input_audio_tokens")),
            cache_audio_read_tokens: UsageNumber::Int(int("cache_audio_read_tokens")),
            output_audio_tokens: UsageNumber::Int(int("output_audio_tokens")),
            details: usage
                .get("details")
                .and_then(Value::as_object)
                .cloned()
                .unwrap(),
        }
    }

    /// Stored finish marker as the lane `Finish` (the stream never
    /// yields `error`: no capture stores it on a streamed response).
    fn finish_from_stored(message: &Value, context: &str) -> Option<Finish> {
        match message.get("finish_reason") {
            None | Some(Value::Null) => None,
            Some(Value::String(raw)) => match raw.as_str() {
                "stop" => Some(Finish::Stop),
                "length" => Some(Finish::Length),
                "tool_call" => Some(Finish::ToolCall),
                "content_filter" => Some(Finish::ContentFilter),
                other => panic!("{context} unexpected stored finish {other}"),
            },
            other => panic!("{context} unexpected finish shape {other:?}"),
        }
    }

    fn assert_tool_args_equal(want: &Value, got: &Option<ToolArgs>, context: &str) {
        match (want, got) {
            (Value::Null, None) => {}
            (Value::String(want), Some(ToolArgs::Raw(got))) => {
                assert_eq!(want, got, "{context} raw args verbatim");
            }
            (Value::Object(want), Some(ToolArgs::Dict(got))) => {
                assert_eq!(want, got, "{context} dict args");
            }
            _ => panic!("{context} args shape: want={want} got={got:?}"),
        }
    }

    /// A parsed stream against its stored response message: parts
    /// (text verbatim, calls with verbatim args), usage, finish,
    /// model, response id, and provider details. Stored part
    /// id/provider stamps are null on every capture (neither lane
    /// passes them for these parts) and asserted so.
    fn assert_turned_matches_stored(turned: &StreamedTurn, stored: &Value, context: &str) {
        let parts = stored.get("parts").and_then(Value::as_array).unwrap();
        assert_eq!(turned.parts.len(), parts.len(), "{context} part count");
        for (part, want) in turned.parts.iter().zip(parts.iter()) {
            assert_eq!(
                want.get("id"),
                Some(&Value::Null),
                "{context} stored part id null"
            );
            assert_eq!(
                want.get("provider_name"),
                Some(&Value::Null),
                "{context} stored part provider null"
            );
            assert_eq!(
                want.get("provider_details"),
                Some(&Value::Null),
                "{context} stored part details null"
            );
            match (part, want.get("part_kind").and_then(Value::as_str)) {
                (StreamPart::Text(got), Some("text")) => {
                    assert_eq!(
                        got,
                        want.get("content").and_then(Value::as_str).unwrap(),
                        "{context} text verbatim"
                    );
                }
                (StreamPart::ToolCall(got), Some("tool-call")) => {
                    assert_eq!(
                        got.name,
                        want.get("tool_name").and_then(Value::as_str).unwrap(),
                        "{context} tool name"
                    );
                    assert_eq!(
                        got.call_id,
                        want.get("tool_call_id").and_then(Value::as_str).unwrap(),
                        "{context} tool call id"
                    );
                    assert_tool_args_equal(want.get("args").unwrap(), &got.args, context);
                }
                _ => panic!("{context} part kind: got={part:?} want={want}"),
            }
        }
        assert_eq!(
            turned.usage,
            request_usage_from_stored(stored.get("usage").unwrap()),
            "{context} usage"
        );
        assert!(
            matches!(
                (&turned.finish, finish_from_stored(stored, context)),
                (None, None)
                    | (Some(Finish::Stop), Some(Finish::Stop))
                    | (Some(Finish::Length), Some(Finish::Length))
                    | (Some(Finish::ToolCall), Some(Finish::ToolCall))
                    | (Some(Finish::ContentFilter), Some(Finish::ContentFilter))
            ),
            "{context} finish: got={:?}",
            turned.finish
        );
        assert_eq!(
            turned.model_name,
            stored.get("model_name").and_then(Value::as_str).unwrap(),
            "{context} model name"
        );
        assert_eq!(
            turned.response_id.as_deref(),
            stored.get("provider_response_id").and_then(Value::as_str),
            "{context} response id"
        );
        let want_details = match stored.get("provider_details") {
            None | Some(Value::Null) => None,
            Some(Value::Object(map)) => Some(map.clone()),
            other => panic!("{context} details shape {other:?}"),
        };
        assert_eq!(turned.provider_details, want_details, "{context} details");
    }

    /// Live text events rebuild the final text parts: starts open
    /// each part at its index, deltas append in order.
    fn assert_events_rebuild_text(events: &[StreamEvent], turned: &StreamedTurn, context: &str) {
        let mut texts: Vec<String> = Vec::new();
        for event in events {
            match event {
                StreamEvent::TextStart { index, initial } => {
                    assert_eq!(*index, texts.len(), "{context} start opens the next part");
                    texts.push(initial.clone());
                }
                StreamEvent::TextDelta { index, delta } => {
                    assert!(*index < texts.len(), "{context} delta targets an open part");
                    texts[*index].push_str(delta);
                }
            }
        }
        let want: Vec<&str> = turned
            .parts
            .iter()
            .filter_map(|part| match part {
                StreamPart::Text(text) => Some(text.as_str()),
                StreamPart::ToolCall(_) => None,
            })
            .collect();
        assert_eq!(texts, want, "{context} events rebuild text parts");
    }

    /// Every captured 200 stream parses to its stored response —
    /// whole-feed and 7-byte feeds alike — with live events that
    /// rebuild the text parts.
    #[test]
    fn parsed_streams_match_stored_responses() {
        let table = provider_table();
        // (fixture, lane, exchange -> stored response lookup)
        for name in [
            "text_openai",
            "tool_local",
            "tool_mcp",
            "retry_validation",
            "replay_3turn",
            "text_anthropic",
            "tool_anthropic",
        ] {
            let fixture = load_fixture(name);
            let lane = if name.contains("anthropic") {
                "anthropic"
            } else {
                "openai"
            };
            let exchanges = fixture
                .get("provider_exchanges")
                .and_then(Value::as_array)
                .unwrap();
            for (index, exchange) in exchanges.iter().enumerate() {
                let context = format!("{name}#{index}");
                let status = exchange
                    .get("response")
                    .and_then(|r| r.get("status"))
                    .and_then(Value::as_u64)
                    .unwrap();
                assert_eq!(status, 200, "{context} streams only");
                let sse = exchange
                    .get("response")
                    .and_then(|r| r.get("sse_text"))
                    .and_then(Value::as_str)
                    .unwrap();
                let stored = if name == "replay_3turn" {
                    fixture.get("turns").and_then(Value::as_array).unwrap()[index]
                        .get("model_messages")
                        .and_then(Value::as_array)
                        .unwrap()[1]
                        .clone()
                } else {
                    fixture
                        .get("model_messages")
                        .and_then(Value::as_array)
                        .unwrap()[index * 2 + 1]
                        .clone()
                };
                assert_eq!(
                    stored.get("kind").and_then(Value::as_str),
                    Some("response"),
                    "{context} stored response"
                );
                let provider_url = "http://stub/v1/";
                let model = exchange
                    .get("request")
                    .and_then(|r| r.get("body"))
                    .and_then(|b| b.get("model"))
                    .and_then(Value::as_str)
                    .unwrap();
                for (feed_name, feeds) in [
                    ("whole", vec![sse.as_bytes().to_vec()]),
                    (
                        "chunked",
                        sse.as_bytes().chunks(7).map(<[u8]>::to_vec).collect(),
                    ),
                ] {
                    let context = format!("{context}/{feed_name}");
                    let (events, turned) = if lane == "openai" {
                        let mut stream = OpenAIStream::new(&table, provider_url, model);
                        let mut events = Vec::new();
                        for feed in &feeds {
                            events.extend(stream.feed(feed).unwrap());
                        }
                        (events, stream.finish().unwrap())
                    } else {
                        let mut stream = AnthropicStream::new(&table, provider_url, model);
                        let mut events = Vec::new();
                        for feed in &feeds {
                            events.extend(stream.feed(feed).unwrap());
                        }
                        (events, stream.finish().unwrap())
                    };
                    assert_turned_matches_stored(&turned, &stored, &context);
                    assert_events_rebuild_text(&events, &turned, &context);
                }
            }
        }
    }

    /// An Anthropic `tool_use` block with zero `input_json` deltas
    /// keeps `args: None` (the stored null,
    /// `anthropic.py:2222`) — no capture hits this shape, so a
    /// trimmed synthetic stream pins it.
    #[test]
    fn anthropic_tool_use_without_deltas_keeps_null_args() {
        let table = provider_table();
        let sse = concat!(
            "event: message_start\n",
            "data: {\"type\": \"message_start\", \"message\": {\"id\": \"m1\", \"model\": \"c\", \"usage\": {\"input_tokens\": 3, \"output_tokens\": 1}}}\n",
            "\n",
            "event: content_block_start\n",
            "data: {\"type\": \"content_block_start\", \"index\": 0, \"content_block\": {\"type\": \"tool_use\", \"id\": \"t1\", \"name\": \"list_projects\", \"input\": {}}}\n",
            "\n",
            "event: content_block_stop\n",
            "data: {\"type\": \"content_block_stop\", \"index\": 0}\n",
            "\n",
            "event: message_delta\n",
            "data: {\"type\": \"message_delta\", \"delta\": {\"stop_reason\": \"tool_use\"}, \"usage\": {\"output_tokens\": 2}}\n",
            "\n",
            "event: message_stop\n",
            "data: {\"type\": \"message_stop\"}\n",
            "\n",
        );
        let mut stream = AnthropicStream::new(&table, "https://api.anthropic.com/", "c");
        let events = stream.feed(sse.as_bytes()).unwrap();
        assert!(events.is_empty(), "no text events");
        let turned = stream.finish().unwrap();
        assert_eq!(turned.parts.len(), 1);
        let StreamPart::ToolCall(call) = &turned.parts[0] else {
            panic!("streams a tool call");
        };
        assert_eq!(call.name, "list_projects");
        assert_eq!(call.call_id, "t1");
        assert!(call.args.is_none(), "zero deltas keep null args");
        assert!(matches!(turned.finish, Some(Finish::ToolCall)));
        assert_eq!(turned.usage.input_tokens, UsageNumber::Int(3));
        assert_eq!(turned.usage.output_tokens, UsageNumber::Int(2));
    }

    /// Fragmented OpenAI tool calls assemble across chunks (name and
    /// argument fragments append; ids replace), calls stay in
    /// creation order, and a nameless slot is dropped from the final
    /// parts like `get_parts`.
    #[test]
    fn openai_fragmented_tool_calls_assemble() {
        let table = provider_table();
        let chunk = |delta: &str, finish: &str| {
            format!(
                "data: {{\"id\": \"s\", \"model\": \"m\", \"choices\": [{{\"index\": 0, \"delta\": {delta}, \"finish_reason\": {finish}}}] }}\n\n"
            )
        };
        let sse = format!(
            "{}{}{}data: [DONE]\n\n",
            chunk(
                r#"{"tool_calls": [{"index": 1, "id": "call_b", "function": {"name": "second", "arguments": "{\"x\":"}}]}"#,
                "null"
            ),
            chunk(
                r#"{"content": "hi", "tool_calls": [{"index": 0, "function": {"name": "fi"}}, {"index": 1, "function": {"arguments": "1}"}}]}"#,
                "null"
            ),
            chunk(
                r#"{"tool_calls": [{"index": 0, "id": "call_a", "function": {"name": "rst", "arguments": "{}"}}]}"#,
                "\"tool_calls\""
            ),
        );
        let mut stream = OpenAIStream::new(&table, "http://stub/v1/", "m");
        let mut events = Vec::new();
        for feed in sse.as_bytes().chunks(11) {
            events.extend(stream.feed(feed).unwrap());
        }
        // Feeding after [DONE] is a no-op.
        assert!(stream.feed(b"data: {}\n\n").unwrap().is_empty());
        let turned = stream.finish().unwrap();
        assert_eq!(
            events,
            vec![StreamEvent::TextStart {
                index: 1,
                initial: "hi".to_owned()
            }],
            "text opens part 1 (tool slot 1 came first)"
        );
        assert_eq!(turned.parts.len(), 3);
        let StreamPart::ToolCall(second) = &turned.parts[0] else {
            panic!("first part is the index-1 call");
        };
        assert_eq!(second.name, "second");
        assert_eq!(second.call_id, "call_b");
        assert!(matches!(&second.args, Some(ToolArgs::Raw(args)) if args == "{\"x\":1}"));
        assert!(matches!(&turned.parts[1], StreamPart::Text(text) if text == "hi"));
        let StreamPart::ToolCall(first) = &turned.parts[2] else {
            panic!("third part is the index-0 call");
        };
        assert_eq!(first.name, "first");
        assert_eq!(first.call_id, "call_a");
        assert!(matches!(turned.finish, Some(Finish::ToolCall)));

        // A nameless slot never surfaces.
        let mut stream = OpenAIStream::new(&table, "http://stub/v1/", "m");
        stream
            .feed(
                chunk(r#"{"tool_calls": [{"index": 0, "id": "call_x", "function": {"arguments": "{}"}}]}"#,
                    "null")
                .as_bytes(),
            )
            .unwrap();
        assert!(
            stream.finish().unwrap().parts.is_empty(),
            "nameless slots are dropped"
        );

        // A generated id fills a missing one (`as_part`).
        let mut stream = OpenAIStream::new(&table, "http://stub/v1/", "m");
        stream
            .feed(
                chunk(
                    r#"{"tool_calls": [{"index": 0, "function": {"name": "n", "arguments": "{}"}}]}"#,
                    "null",
                )
                .as_bytes(),
            )
            .unwrap();
        let turned = stream.finish().unwrap();
        let StreamPart::ToolCall(call) = &turned.parts[0] else {
            panic!("call assembles");
        };
        assert!(
            call.call_id.starts_with("pyd_ai_"),
            "generated call id: {}",
            call.call_id
        );
    }

    /// A truthy first-chunk `created` lands in provider details
    /// as the UTC ISO `timestamp` pydantic's JSON mode emits
    /// (`openai.py:1198,3232-3234`); a falsy `created` — every
    /// oracle capture — leaves the key unset. Only the first
    /// chunk's stamp counts (the SDK peeks it once).
    #[test]
    fn openai_created_stamp_lands_in_details() {
        let table = provider_table();
        let chunk = |created: i64| {
            format!(
                "data: {{\"id\": \"s\", \"created\": {created}, \"model\": \"m\", \"choices\": [{{\"index\": 0, \"delta\": {{\"content\": \"ok\"}}, \"finish_reason\": \"stop\"}}] }}\n\n"
            )
        };
        let mut stream = OpenAIStream::new(&table, "http://stub/v1/", "m");
        stream.feed(chunk(1760716800).as_bytes()).unwrap();
        stream.feed(b"data: [DONE]\n\n").unwrap();
        let turned = stream.finish().unwrap();
        let details = turned.provider_details.unwrap();
        assert_eq!(
            details.get("timestamp").and_then(Value::as_str),
            Some("2025-10-17T16:00:00Z"),
            "probed pydantic JSON bytes"
        );
        assert_eq!(
            details.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["timestamp", "finish_reason"],
            "timestamp merges first"
        );
        // Falsy first stamp: no key, even when a later chunk carries one.
        let mut stream = OpenAIStream::new(&table, "http://stub/v1/", "m");
        stream.feed(chunk(0).as_bytes()).unwrap();
        stream.feed(chunk(1760716800).as_bytes()).unwrap();
        let turned = stream.finish().unwrap();
        assert_eq!(
            turned.provider_details,
            Some(Map::from_iter([(
                "finish_reason".to_owned(),
                Value::String("stop".to_owned())
            )])),
            "first chunk decides"
        );
    }

    /// An empty tool-call id never replaces the stored one (`if
    /// self.tool_call_id:`, `messages.py:2656,2687`).
    #[test]
    fn empty_tool_call_id_does_not_replace() {
        let mut manager = PartsManager::default();
        manager
            .tool_delta(VendorId::Index(0), Some("n"), None, Some("call_a"))
            .unwrap();
        manager
            .tool_delta(VendorId::Index(0), None, None, Some(""))
            .unwrap();
        let parts = manager.parts();
        assert_eq!(parts.len(), 1);
        let StreamPart::ToolCall(call) = &parts[0] else {
            panic!("one call");
        };
        assert_eq!(call.call_id, "call_a");
    }

    /// Streams with zero data dispatches raise exactly like the
    /// SDK's empty-stream peek (`openai.py:1189-1192`,
    /// `anthropic.py:980-982`), and classify `internal`.
    /// `[DONE]`-only, comment-only, and ping-only streams all count
    /// as empty (none yields a reader item).
    #[test]
    fn empty_streams_raise() {
        let table = provider_table();
        let openai = Lane::OpenAICompatible {
            model: "m".to_owned(),
            base_url: "http://stub/v1".to_owned(),
            api_key: "k".to_owned(),
        };
        for (name, bytes) in [
            ("empty", b"".as_slice()),
            ("done-only", b"data: [DONE]\n\n".as_slice()),
            ("comment-only", b": heartbeat\n\n".as_slice()),
        ] {
            let mut stream = OpenAIStream::new(&table, "http://stub/v1/", "m");
            assert!(stream.feed(bytes).unwrap().is_empty(), "{name} no events");
            let error = stream.finish().unwrap_err();
            assert!(
                matches!(&error, ProviderError::InvalidResponse(message) if message == "Streamed response ended without content or tool calls"),
                "{name}: {error:?}"
            );
            assert_eq!(
                classify_provider_error(&openai, &error),
                None,
                "{name} classifies internal"
            );
        }
        let anthropic = Lane::Anthropic {
            model: "c".to_owned(),
            api_key: "k".to_owned(),
        };
        for (name, bytes) in [
            ("empty", b"".as_slice()),
            ("ping-only", b"event: ping\ndata: {}\n\n".as_slice()),
        ] {
            let mut stream = AnthropicStream::new(&table, "https://api.anthropic.com/", "c");
            assert!(stream.feed(bytes).unwrap().is_empty(), "{name} no events");
            let error = stream.finish().unwrap_err();
            assert!(
                matches!(&error, ProviderError::InvalidResponse(message) if message == "Streamed response ended without content or tool calls"),
                "{name}: {error:?}"
            );
            assert_eq!(
                classify_provider_error(&anthropic, &error),
                None,
                "{name} classifies internal"
            );
        }
    }

    /// Mixed string/dict argument deltas raise, like
    /// `ToolCallPartDelta.apply`.
    #[test]
    fn mixed_args_deltas_raise() {
        let mut manager = PartsManager::default();
        manager
            .tool_delta(
                VendorId::Index(0),
                Some("n"),
                Some(ToolArgsDelta::Str("{".to_owned())),
                None,
            )
            .unwrap();
        assert!(
            manager
                .tool_delta(
                    VendorId::Index(0),
                    None,
                    Some(ToolArgsDelta::Dict(Map::new())),
                    None,
                )
                .is_err(),
            "dict over string raises"
        );
        let mut manager = PartsManager::default();
        manager
            .tool_delta(
                VendorId::Index(0),
                Some("n"),
                Some(ToolArgsDelta::Dict(Map::new())),
                None,
            )
            .unwrap();
        assert!(
            manager
                .tool_delta(
                    VendorId::Index(0),
                    None,
                    Some(ToolArgsDelta::Str("}".to_owned())),
                    None,
                )
                .is_err(),
            "string over dict raises"
        );
    }

    /// Mid-stream error payloads surface with the SDK texts and
    /// classify with no status: the OpenAI `APIError` message, the
    /// Anthropic `str(body)` / raw data.
    #[test]
    fn mid_stream_errors() {
        let table = provider_table();
        let openai = Lane::OpenAICompatible {
            model: "m".to_owned(),
            base_url: "http://stub/v1".to_owned(),
            api_key: "k".to_owned(),
        };
        let mut stream = OpenAIStream::new(&table, "http://stub/v1/", "m");
        let error = stream
            .feed(b"data: {\"error\": {\"message\": \"boom\", \"type\": \"x\"}}\n\n")
            .unwrap_err();
        assert!(
            matches!(&error, ProviderError::InvalidResponse(message) if message == "boom"),
            "openai error message: {error:?}"
        );
        assert_eq!(classify_provider_error(&openai, &error), None);
        let mut stream = OpenAIStream::new(&table, "http://stub/v1/", "m");
        let error = stream.feed(b"data: {\"error\": {}}\n\n").unwrap_err();
        assert!(
            matches!(&error, ProviderError::InvalidResponse(message) if message == "An error occurred during streaming"),
            "openai default message: {error:?}"
        );

        let anthropic = Lane::Anthropic {
            model: "c".to_owned(),
            api_key: "k".to_owned(),
        };
        let mut stream = AnthropicStream::new(&table, "https://api.anthropic.com/", "c");
        let error = stream
            .feed(b"event: error\ndata: {\"type\": \"error\", \"error\": {\"type\": \"overloaded\", \"message\": \"busy\"}}\n\n")
            .unwrap_err();
        let ProviderError::InvalidResponse(message) = &error else {
            panic!("anthropic error event: {error:?}");
        };
        assert_eq!(
            message, "{'type': 'error', 'error': {'type': 'overloaded', 'message': 'busy'}}",
            "python str of the parsed body"
        );
        assert_eq!(classify_provider_error(&anthropic, &error), None);
        let mut stream = AnthropicStream::new(&table, "https://api.anthropic.com/", "c");
        let error = stream
            .feed(b"event: error\ndata: not json\n\n")
            .unwrap_err();
        assert!(
            matches!(&error, ProviderError::InvalidResponse(message) if message == "not json"),
            "raw data passthrough: {error:?}"
        );
    }

    /// The captured 401/402 failures classify to the exact codes
    /// Django recorded, with the exact `ModelHTTPError` message
    /// text. That text is `str()` of pydantic-ai's exception, whose
    /// body the OpenAI SDK already unwrapped (`body["error"]`); the
    /// raw response envelope in the fixture is NOT what `str(exc)`
    /// shows (probed live against pinned pydantic-ai 1.107.0 +
    /// openai 2.29.0, PIDASHCONV-1536).
    #[test]
    fn error_exchanges_classify_like_django() {
        let lane = Lane::OpenAICompatible {
            model: "oracle-stub".to_owned(),
            base_url: "http://stub/v1".to_owned(),
            api_key: "k".to_owned(),
        };
        for (name, want_message) in [
            (
                "error_401",
                "status_code: 401, model_name: oracle-stub, body: {'message': 'Incorrect API key provided.', 'type': 'invalid_api_key', 'code': 'invalid_api_key'}",
            ),
            (
                "error_402",
                "status_code: 402, model_name: oracle-stub, body: {'message': 'Insufficient credits. Top up at https://example.test/billing.', 'type': 'insufficient_credits', 'code': 402}",
            ),
        ] {
            let fixture = load_fixture(name);
            let exchange = &fixture
                .get("provider_exchanges")
                .and_then(Value::as_array)
                .unwrap()[0];
            let status = exchange
                .get("response")
                .and_then(|r| r.get("status"))
                .and_then(Value::as_u64)
                .unwrap() as u16;
            let body = exchange
                .get("response")
                .and_then(|r| r.get("body"))
                .unwrap()
                .clone();
            let error = ProviderError::Http {
                status,
                body_text: serde_json::to_string(&body).unwrap(),
                body_json: Some(body),
            };
            assert_eq!(error.message(&lane), want_message, "{name} message text");
            let turn = fixture.get("turn").unwrap();
            let (code, detail) = classify_provider_error(&lane, &error)
                .unwrap_or_else(|| panic!("{name} classifies"));
            assert_eq!(
                (code.as_str(), detail.as_str()),
                (
                    turn.get("error_code").and_then(Value::as_str).unwrap(),
                    turn.get("error_detail").and_then(Value::as_str).unwrap()
                ),
                "{name} code and detail"
            );
        }
    }

    /// Timeouts, connections, and malformed payloads classify on
    /// text alone, on both lanes.
    #[test]
    fn transport_errors_classify() {
        let openai = Lane::OpenAICompatible {
            model: "m".to_owned(),
            base_url: "http://stub/v1".to_owned(),
            api_key: "k".to_owned(),
        };
        let anthropic = Lane::Anthropic {
            model: "c".to_owned(),
            api_key: "k".to_owned(),
        };
        for lane in [&openai, &anthropic] {
            assert_eq!(
                classify_provider_error(lane, &ProviderError::Timeout)
                    .as_ref()
                    .map(|(code, detail)| (code.as_str(), detail.as_str())),
                Some((
                    "provider_unreachable",
                    "Could not reach the configured provider endpoint."
                ))
            );
            assert_eq!(
                classify_provider_error(lane, &ProviderError::Connection)
                    .as_ref()
                    .map(|(code, detail)| (code.as_str(), detail.as_str())),
                Some((
                    "provider_unreachable",
                    "Could not reach the configured provider endpoint."
                ))
            );
            assert_eq!(
                classify_provider_error(
                    lane,
                    &ProviderError::InvalidResponse("bad payload".to_owned())
                ),
                None
            );
        }
        // A 402 hiding in message text (request ids, token counts)
        // must not misclassify: only the status counts.
        assert_eq!(
            classify_provider_error(
                &openai,
                &ProviderError::InvalidResponse("request 402abc failed".to_owned())
            ),
            None
        );
        // Unknown-model text still maps, naming the lane's model.
        assert_eq!(
            classify_provider_error(
                &openai,
                &ProviderError::Http {
                    status: 404,
                    body_text: "model_not_found".to_owned(),
                    body_json: None,
                }
            )
            .as_ref()
            .map(|(code, detail)| (code.as_str(), detail.as_str())),
            Some((
                "model_invalid",
                "The model \"m\" was not accepted by the provider. Check the model name in Settings."
            ))
        );
    }

    /// Usage addition: six counters plus the numeric details merge;
    /// `output_audio_tokens` never accumulates; bools count as ints;
    /// non-numeric values and incumbents are left alone; int+int
    /// stays int.
    #[test]
    fn usage_addition_merges() {
        let left = RequestUsage {
            input_tokens: UsageNumber::Int(10),
            output_tokens: UsageNumber::Int(5),
            output_audio_tokens: UsageNumber::Int(7),
            details: Map::from_iter([
                ("cached".to_owned(), Value::Number(3.into())),
                ("note".to_owned(), Value::String("keep".to_owned())),
                ("flag".to_owned(), Value::Bool(true)),
            ]),
            ..RequestUsage::zeros()
        };
        let right = RequestUsage {
            input_tokens: UsageNumber::Int(2),
            output_audio_tokens: UsageNumber::Int(100),
            details: Map::from_iter([
                ("cached".to_owned(), Value::Number(4.into())),
                ("note".to_owned(), Value::Number(9.into())),
                ("flag".to_owned(), Value::Bool(true)),
                ("fresh".to_owned(), Value::Number(1.into())),
                ("skip".to_owned(), Value::String("x".to_owned())),
            ]),
            ..RequestUsage::zeros()
        };
        let merged = add_request_usage(&left, &right);
        assert_eq!(merged.input_tokens, UsageNumber::Int(12));
        assert_eq!(merged.output_tokens, UsageNumber::Int(5));
        assert_eq!(
            merged.output_audio_tokens,
            UsageNumber::Int(7),
            "output_audio never accumulates"
        );
        assert_eq!(merged.details.get("cached"), Some(&Value::Number(7.into())));
        assert_eq!(
            merged.details.get("note"),
            Some(&Value::String("keep".to_owned())),
            "non-numeric incumbent kept"
        );
        assert_eq!(
            merged.details.get("flag"),
            Some(&Value::Number(2.into())),
            "bools count as ints"
        );
        assert_eq!(merged.details.get("fresh"), Some(&Value::Number(1.into())));
        assert!(!merged.details.contains_key("skip"));
        // Float mixing.
        let left = RequestUsage {
            input_tokens: UsageNumber::Float(1.5),
            ..RequestUsage::zeros()
        };
        let right = RequestUsage {
            input_tokens: UsageNumber::Int(1),
            ..RequestUsage::zeros()
        };
        assert_eq!(
            add_request_usage(&left, &right).input_tokens,
            UsageNumber::Float(2.5)
        );
    }

    /// Tool-result rendering per lane: OpenAI's
    /// `model_response_str` (null and `[]` read `""`; non-empty
    /// lists stay lists) and Anthropic's `content_items(mode='str')`
    /// (null renders `"null"`; `[]` yields no items).
    #[test]
    fn tool_result_rendering_cases() {
        assert_eq!(tool_response_str(&Value::Null), "");
        assert_eq!(tool_response_str(&Value::Array(vec![])), "");
        assert_eq!(tool_response_str(&Value::String("a".to_owned())), "a");
        assert_eq!(
            tool_response_str(&serde_json::json!(["a", "b"])),
            r#"["a","b"]"#
        );
        assert_eq!(tool_response_str(&serde_json::json!(["a"])), r#"["a"]"#);
        assert_eq!(tool_response_str(&serde_json::json!(5)), "5");
        assert_eq!(tool_result_item_text(&Value::Null), "null");
        assert_eq!(tool_result_item_text(&Value::String("a".to_owned())), "a");
        assert_eq!(tool_result_item_text(&serde_json::json!(5)), "5");
        // Anthropic `content or ''`.
        let empty = anthropic_tool_result("t", &Value::Array(vec![]));
        assert_eq!(
            empty.get("content").and_then(Value::as_str),
            Some(""),
            "empty items send the empty string"
        );
        let nulled = anthropic_tool_result("t", &Value::Null);
        assert_eq!(
            nulled.get("content"),
            Some(&Value::Array(vec![anthropic_text_block_text_first("null")])),
            "null is one null item"
        );
    }

    /// Argument replay shapes: `args_as_json_str` (falsy to `'{}'`,
    /// raw verbatim, dicts compact) and `args_as_dict` (falsy to
    /// `{}`, strings parsed, the `INVALID_JSON` wrap otherwise).
    #[test]
    fn tool_args_replay_cases() {
        assert_eq!(tool_args_json_str(None), "{}");
        assert_eq!(
            tool_args_json_str(Some(&ToolArgs::Raw(String::new()))),
            "{}"
        );
        assert_eq!(
            tool_args_json_str(Some(&ToolArgs::Raw(r#"{"b": 1}"#.to_owned()))),
            r#"{"b": 1}"#
        );
        assert_eq!(tool_args_dict(None), Map::new());
        assert_eq!(
            tool_args_dict(Some(&ToolArgs::Raw(String::new()))),
            Map::new()
        );
        assert_eq!(
            tool_args_dict(Some(&ToolArgs::Raw(r#"{"b":1}"#.to_owned()))),
            Map::from_iter([("b".to_owned(), Value::Number(1.into()))])
        );
        assert_eq!(
            tool_args_dict(Some(&ToolArgs::Raw("xx".to_owned()))),
            Map::from_iter([(
                pidash_jobs::oss_worker::model_runtime::provider::INVALID_JSON_KEY.to_owned(),
                Value::String("xx".to_owned())
            )])
        );
    }

    /// Endpoint shapes: the Anthropic beta query, the OpenAI base
    /// join.
    #[test]
    fn endpoint_shapes() {
        let anthropic = Lane::Anthropic {
            model: "c".to_owned(),
            api_key: "k".to_owned(),
        };
        assert_eq!(
            stream_endpoint(&anthropic),
            "https://api.anthropic.com/v1/messages?beta=true"
        );
        let openai = Lane::OpenAICompatible {
            model: "m".to_owned(),
            base_url: "http://stub/v1/".to_owned(),
            api_key: "k".to_owned(),
        };
        assert_eq!(stream_endpoint(&openai), "http://stub/v1/chat/completions");
    }

    /// SSE framing: `\r\n` endings, comment lines, multi-`data`
    /// joins, partial feeds, and data-less dispatches dropped.
    #[test]
    fn sse_framing_cases() {
        let mut framer = SseFramer::default();
        framer.feed(b"event: message_start\r\ndata: {\"a\":1}\r\n\r\n: comment\n\n");
        framer.feed(b"data: one\ndata: two\n\n");
        let events = framer.drain();
        assert_eq!(
            events,
            vec![
                SseEvent {
                    name: "message_start".to_owned(),
                    data: "{\"a\":1}".to_owned()
                },
                SseEvent {
                    name: "message".to_owned(),
                    data: "one\ntwo".to_owned()
                },
            ]
        );
        // Partial feeds hold the dispatch until the blank line.
        framer.feed(b"data: partial");
        assert!(framer.drain().is_empty());
        framer.feed(b"\n\n");
        assert_eq!(framer.drain().len(), 1);
    }

    /// History inputs for one exchange, mirroring the byte tests:
    /// stored prefixes, per-turn replay blobs, the error fixtures'
    /// user row, or the chained usage-limit walk. Returns `None`
    /// for `usage_limit`, whose history the caller chains.
    fn exchange_history(name: &str, index: usize, fixture: &Value) -> Option<Vec<TurnMessage>> {
        if name == "usage_limit" {
            return None;
        }
        if name == "replay_3turn" {
            let histories = stored_histories(fixture);
            let mut history: Vec<TurnMessage> = histories
                .iter()
                .take(index)
                .flat_map(|blob| blob.iter().cloned())
                .collect();
            history.push(histories[index][0].clone());
            return Some(history);
        }
        let histories = stored_histories(fixture);
        if histories.is_empty() {
            let user_text = fixture
                .get("messages")
                .and_then(Value::as_array)
                .unwrap()
                .iter()
                .find(|m| m.get("kind").and_then(Value::as_str) == Some("user"))
                .and_then(|m| m.get("display_content"))
                .and_then(Value::as_str)
                .unwrap()
                .to_owned();
            return Some(vec![TurnMessage::Request {
                parts: vec![TurnRequestPart::User {
                    text: user_text,
                    timestamp: None,
                }],
                timestamp: None,
                instructions: None,
                run_id: None,
                conversation_id: None,
                metadata: Value::Null,
            }]);
        }
        Some(histories[0][..index * 2 + 1].to_vec())
    }

    /// One request a stub server recorded: method, path plus query,
    /// the meaningful headers, and the raw body bytes.
    #[derive(Debug, Clone)]
    struct StubRequest {
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    }

    /// One scripted stub reply: status plus raw bytes (SSE verbatim,
    /// JSON re-encoded — the captures pin error bodies parsed, not
    /// bytewise) and optional extra headers.
    #[derive(Debug, Clone)]
    struct StubReply {
        status: u16,
        headers: Vec<(String, String)>,
        bytes: Vec<u8>,
        /// Sleep before replying (timeout probes).
        sleep_secs: u64,
    }

    #[derive(Clone)]
    struct StubState {
        requests: std::sync::Arc<std::sync::Mutex<Vec<StubRequest>>>,
        script: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<StubReply>>>,
    }

    async fn stub_handler(
        axum::extract::State(state): axum::extract::State<StubState>,
        req: axum::http::Request<axum::body::Body>,
    ) -> impl axum::response::IntoResponse {
        use axum::body::to_bytes;
        let method = req.method().to_string();
        let path = req
            .uri()
            .path_and_query()
            .map(|pq| pq.to_string())
            .unwrap_or_default();
        let mut headers = HashMap::new();
        for name in [
            "content-type",
            "content-length",
            "user-agent",
            "authorization",
            "x-api-key",
            "anthropic-version",
        ] {
            if let Some(value) = req.headers().get(name).and_then(|v| v.to_str().ok()) {
                headers.insert(name.to_owned(), value.to_owned());
            }
        }
        let body = to_bytes(req.into_body(), usize::MAX).await.unwrap();
        state.requests.lock().unwrap().push(StubRequest {
            method,
            path,
            headers,
            body: body.to_vec(),
        });
        let reply = state.script.lock().unwrap().pop_front().unwrap();
        if reply.sleep_secs > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(reply.sleep_secs)).await;
        }
        let mut response = axum::response::Response::builder().status(reply.status);
        for (name, value) in &reply.headers {
            response = response.header(name, value);
        }
        response.body(axum::body::Body::from(reply.bytes)).unwrap()
    }

    async fn stub_server(script: Vec<StubReply>) -> (String, StubState) {
        let state = StubState {
            requests: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            script: std::sync::Arc::new(std::sync::Mutex::new(script.into())),
        };
        let app = axum::Router::new()
            .fallback(stub_handler)
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}"), state)
    }

    fn stub_body(name: &str, exchange: &Value) -> Value {
        exchange
            .get("request")
            .and_then(|r| r.get("body"))
            .unwrap_or_else(|| panic!("{name} captures the request body"))
            .clone()
    }

    /// Lane plus history plus wire body for one exchange: what the
    /// agent loop hands [`stream_turn`].
    fn exchange_inputs(
        name: &str,
        index: usize,
        fixture: &Value,
        history: &[TurnMessage],
        stub_base: &str,
    ) -> (Lane, Value) {
        let lane_name = if name.contains("anthropic") {
            "anthropic"
        } else {
            "openai"
        };
        let body = stub_body(name, &fixture["provider_exchanges"][index]);
        let model = body.get("model").and_then(Value::as_str).unwrap();
        let tools = wire_tools_from_body(&body);
        let built = if lane_name == "openai" {
            openai_request_body(
                model,
                &openai_instructions_from_body(&body),
                history,
                &tools,
            )
        } else {
            anthropic_request_body(
                model,
                &anthropic_instructions_from_body(&body),
                history,
                &tools,
            )
        };
        let lane = if lane_name == "openai" {
            Lane::OpenAICompatible {
                model: model.to_owned(),
                base_url: format!("{stub_base}/v1"),
                api_key: "test-key".to_owned(),
            }
        } else {
            Lane::Anthropic {
                model: model.to_owned(),
                api_key: "test-key".to_owned(),
            }
        };
        (lane, built)
    }

    /// The stub saw the captured request: method, path (including
    /// the Anthropic `?beta=true`), body bytes, and the meaningful
    /// headers. Transport headers (`host`, `accept`, `connection`)
    /// are the HTTP stack's, not the request's, and not asserted —
    /// like the deliberately unsent `X-Stainless-*` telemetry.
    fn assert_stub_saw_exchange(
        name: &str,
        index: usize,
        seen: &StubRequest,
        exchange: &Value,
        history: &[TurnMessage],
        lane_name: &str,
        lane: &Lane,
    ) {
        let request = exchange.get("request").unwrap();
        assert_eq!(seen.method, "POST", "{name}#{index} method");
        assert_eq!(
            seen.path,
            request.get("path").and_then(Value::as_str).unwrap(),
            "{name}#{index} path"
        );
        let seen_text = String::from_utf8_lossy(&seen.body).into_owned();
        // Seen bytes against the capture: the byte tests already pin
        // builder output, so this proves the HTTP layer sends it
        // through untouched.
        let mut want: Value =
            serde_json::from_str(&exchange_body_bytes(exchange, name, index)).unwrap();
        let mut got: Value = serde_json::from_str(&seen_text).unwrap();
        let pairs = substitute_tool_contents(&mut want, &mut got, history, lane_name, name, index);
        assert!(
            encode_body(&want) == encode_body(&got),
            "{name}#{index} stub saw captured bytes:\n{}",
            byte_diff(&encode_body(&want), &encode_body(&got))
        );
        for (at, (want_content, got_content)) in pairs.iter().enumerate() {
            assert_eq!(
                normalize_json_strings(want_content),
                normalize_json_strings(got_content),
                "{name}#{index} tool content {at} value-equal"
            );
        }
        assert_eq!(
            seen.headers.get("content-type").map(String::as_str),
            Some("application/json"),
            "{name}#{index} content type"
        );
        assert_eq!(
            seen.headers.get("user-agent").map(String::as_str),
            Some("pydantic-ai/1.107.0"),
            "{name}#{index} user agent"
        );
        assert_eq!(
            seen.headers
                .get("content-length")
                .and_then(|v| v.parse::<usize>().ok()),
            Some(seen.body.len()),
            "{name}#{index} content length"
        );
        match lane {
            Lane::OpenAICompatible { .. } => {
                assert_eq!(
                    seen.headers.get("authorization").map(String::as_str),
                    Some("Bearer test-key"),
                    "{name}#{index} bearer auth"
                );
            }
            Lane::Anthropic { .. } => {
                assert_eq!(
                    seen.headers.get("x-api-key").map(String::as_str),
                    Some("test-key"),
                    "{name}#{index} api key"
                );
                assert_eq!(
                    seen.headers.get("anthropic-version").map(String::as_str),
                    Some("2023-06-01"),
                    "{name}#{index} api version"
                );
            }
        }
    }

    /// Every captured provider exchange replays through a stub
    /// server end to end: Rust sends the captured request bytes and
    /// parses the replayed stream/error into the stored parts,
    /// usage, and error codes.
    #[tokio::test]
    async fn stub_server_replays_every_captured_exchange() {
        let table = provider_table();
        let client = reqwest::Client::new();
        let mut chained: Vec<TurnMessage> = Vec::new();
        for name in [
            "text_openai",
            "tool_local",
            "tool_mcp",
            "retry_validation",
            "replay_3turn",
            "text_anthropic",
            "tool_anthropic",
            "error_401",
            "error_402",
            "usage_limit",
        ] {
            let fixture = load_fixture(name);
            let lane_name = if name.contains("anthropic") {
                "anthropic"
            } else {
                "openai"
            };
            let exchanges = fixture
                .get("provider_exchanges")
                .and_then(Value::as_array)
                .unwrap()
                .clone();
            for (index, exchange) in exchanges.iter().enumerate() {
                let context = format!("{name}#{index}");
                // History: stored prefixes, or the chained walk for
                // the usage-limit turn.
                let history = if name == "usage_limit" {
                    if index == 0 {
                        let body = stub_body(name, exchange);
                        let user_text = body
                            .get("messages")
                            .and_then(Value::as_array)
                            .unwrap()
                            .iter()
                            .find(|m| m.get("role").and_then(Value::as_str) == Some("user"))
                            .and_then(|m| m.get("content"))
                            .and_then(Value::as_str)
                            .unwrap()
                            .to_owned();
                        chained = vec![TurnMessage::Request {
                            parts: vec![TurnRequestPart::User {
                                text: user_text,
                                timestamp: None,
                            }],
                            timestamp: None,
                            instructions: None,
                            run_id: None,
                            conversation_id: None,
                            metadata: Value::Null,
                        }];
                    }
                    chained.clone()
                } else {
                    exchange_history(name, index, &fixture).unwrap()
                };
                let response = exchange.get("response").unwrap();
                let status = response.get("status").and_then(Value::as_u64).unwrap() as u16;
                let (reply_bytes, content_type) = match response.get("sse_text") {
                    Some(Value::String(sse)) => (sse.as_bytes().to_vec(), "text/event-stream"),
                    _ => (
                        serde_json::to_vec(response.get("body").unwrap()).unwrap(),
                        "application/json",
                    ),
                };
                let (stub_base, state) = stub_server(vec![StubReply {
                    status,
                    headers: vec![("content-type".to_owned(), content_type.to_owned())],
                    bytes: reply_bytes,
                    sleep_secs: 0,
                }])
                .await;
                let (lane, built) = exchange_inputs(name, index, &fixture, &history, &stub_base);
                let path = exchange
                    .get("request")
                    .and_then(|r| r.get("path"))
                    .and_then(Value::as_str)
                    .unwrap();
                let request = StreamRequest {
                    endpoint_url: format!("{stub_base}{path}"),
                    body: encode_body(&built),
                    timeout_secs: 30,
                    extract: &table,
                    lane: &lane,
                };
                let mut events = Vec::new();
                let outcome = stream_turn(&client, &request, |event| events.push(event)).await;
                let seen = state.requests.lock().unwrap();
                assert_eq!(seen.len(), 1, "{context} one request");
                assert_stub_saw_exchange(
                    name, index, &seen[0], exchange, &history, lane_name, &lane,
                );
                drop(seen);
                if status == 200 {
                    let turned = outcome.unwrap_or_else(|e| panic!("{context} streams: {e:?}"));
                    assert_events_rebuild_text(&events, &turned, &context);
                    if name == "usage_limit" {
                        assert_eq!(turned.parts.len(), 1, "{context} one call");
                        let StreamPart::ToolCall(call) = &turned.parts[0] else {
                            panic!("{context} streams a tool call");
                        };
                        assert_eq!(call.name, "list_projects");
                        assert_eq!(call.call_id, "call_loop");
                        assert!(
                            matches!(&call.args, Some(ToolArgs::Raw(args)) if args == "{}"),
                            "{context} args"
                        );
                        // Extend the chain for the next exchange.
                        chained.push(TurnMessage::Response {
                            parts: vec![TurnResponsePart::ToolCall {
                                name: call.name.clone(),
                                args: call.args.clone(),
                                call_id: call.call_id.clone(),
                                id: None,
                                provider_name: None,
                                provider_details: None,
                            }],
                            usage: RequestUsage::zeros(),
                            model_name: None,
                            timestamp: String::new(),
                            provider_name: None,
                            provider_url: None,
                            provider_details: None,
                            response_id: None,
                            finish: None,
                            run_id: None,
                            conversation_id: None,
                            metadata: Value::Null,
                            state: TurnState::Complete,
                        });
                        chained.push(TurnMessage::Request {
                            parts: vec![TurnRequestPart::ToolReturn {
                                name: call.name.clone(),
                                content: Value::String(String::new()),
                                call_id: call.call_id.clone(),
                                timestamp: None,
                                metadata: Value::Null,
                                outcome: TurnOutcome::Success,
                            }],
                            timestamp: None,
                            instructions: None,
                            run_id: None,
                            conversation_id: None,
                            metadata: Value::Null,
                        });
                    } else {
                        let stored = if name == "replay_3turn" {
                            fixture["turns"][index]["model_messages"][1].clone()
                        } else {
                            fixture["model_messages"][index * 2 + 1].clone()
                        };
                        assert_turned_matches_stored(&turned, &stored, &context);
                    }
                } else {
                    let error = outcome
                        .err()
                        .unwrap_or_else(|| panic!("{context} fails: {status}"));
                    let ProviderError::Http {
                        status: got_status,
                        body_json,
                        ..
                    } = &error
                    else {
                        panic!("{context} http error: {error:?}");
                    };
                    assert_eq!(*got_status, status, "{context} status");
                    assert_eq!(
                        body_json.as_ref().unwrap(),
                        response.get("body").unwrap(),
                        "{context} error body"
                    );
                    let turn = fixture.get("turn").unwrap();
                    let (code, detail) = classify_provider_error(&lane, &error)
                        .unwrap_or_else(|| panic!("{context} classifies"));
                    assert_eq!(
                        (code.as_str(), detail.as_str()),
                        (
                            turn.get("error_code").and_then(Value::as_str).unwrap(),
                            turn.get("error_detail").and_then(Value::as_str).unwrap()
                        ),
                        "{context} code and detail"
                    );
                }
            }
        }
    }

    /// A retriable status retries (honoring `retry-after-ms`) and
    /// the stream then succeeds; the stub saw both requests.
    #[tokio::test]
    async fn stub_server_retries_then_succeeds() {
        let table = provider_table();
        let client = reqwest::Client::new();
        let sse = "data: {\"id\": \"s\", \"model\": \"m\", \"choices\": [{\"index\": 0, \"delta\": {\"content\": \"ok\"}, \"finish_reason\": \"stop\"}]}\n\ndata: [DONE]\n\n";
        let (stub_base, state) = stub_server(vec![
            StubReply {
                status: 429,
                headers: vec![("retry-after-ms".to_owned(), "5".to_owned())],
                bytes: b"{\"error\": \"slow down\"}".to_vec(),
                sleep_secs: 0,
            },
            StubReply {
                status: 200,
                headers: vec![("content-type".to_owned(), "text/event-stream".to_owned())],
                bytes: sse.as_bytes().to_vec(),
                sleep_secs: 0,
            },
        ])
        .await;
        let lane = Lane::OpenAICompatible {
            model: "m".to_owned(),
            base_url: format!("{stub_base}/v1"),
            api_key: "k".to_owned(),
        };
        let request = StreamRequest {
            endpoint_url: format!("{stub_base}/v1/chat/completions"),
            body: encode_body(&serde_json::json!({"model": "m"})),
            timeout_secs: 30,
            extract: &table,
            lane: &lane,
        };
        let mut events = Vec::new();
        let turned = stream_turn(&client, &request, |event| events.push(event))
            .await
            .unwrap();
        assert_eq!(state.requests.lock().unwrap().len(), 2, "one retry");
        assert_eq!(
            events,
            vec![StreamEvent::TextStart {
                index: 0,
                initial: "ok".to_owned()
            }]
        );
        assert!(matches!(&turned.parts[..], [StreamPart::Text(text)] if text == "ok"));
    }

    /// Persistent 500s surface after two retries (three requests);
    /// `x-should-retry: false` disables the retry outright.
    #[tokio::test]
    async fn stub_server_gives_up_after_retries() {
        let table = provider_table();
        let client = reqwest::Client::new();
        let lane = Lane::OpenAICompatible {
            model: "m".to_owned(),
            base_url: "http://stub/v1".to_owned(),
            api_key: "k".to_owned(),
        };
        let failing = StubReply {
            status: 500,
            headers: Vec::new(),
            bytes: b"boom".to_vec(),
            sleep_secs: 0,
        };
        let (stub_base, state) =
            stub_server(vec![failing.clone(), failing.clone(), failing.clone()]).await;
        let request = StreamRequest {
            endpoint_url: format!("{stub_base}/v1/chat/completions"),
            body: encode_body(&serde_json::json!({"model": "m"})),
            timeout_secs: 30,
            extract: &table,
            lane: &lane,
        };
        let error = stream_turn(&client, &request, |_| {}).await.unwrap_err();
        assert!(
            matches!(&error, ProviderError::Http { status: 500, .. }),
            "surfaces the status: {error:?}"
        );
        assert_eq!(state.requests.lock().unwrap().len(), 3, "two retries");

        let (stub_base, state) = stub_server(vec![StubReply {
            status: 500,
            headers: vec![("x-should-retry".to_owned(), "false".to_owned())],
            bytes: b"boom".to_vec(),
            sleep_secs: 0,
        }])
        .await;
        let request = StreamRequest {
            endpoint_url: format!("{stub_base}/v1/chat/completions"),
            body: encode_body(&serde_json::json!({"model": "m"})),
            timeout_secs: 30,
            extract: &table,
            lane: &lane,
        };
        let error = stream_turn(&client, &request, |_| {}).await.unwrap_err();
        assert!(matches!(&error, ProviderError::Http { status: 500, .. }));
        assert_eq!(
            state.requests.lock().unwrap().len(),
            1,
            "override disables retries"
        );
    }

    /// An unresponsive endpoint times out on every attempt and
    /// classifies unreachable.
    #[tokio::test]
    async fn stub_server_timeout_classifies_unreachable() {
        let table = provider_table();
        let client = reqwest::Client::new();
        let lane = Lane::OpenAICompatible {
            model: "m".to_owned(),
            base_url: "http://stub/v1".to_owned(),
            api_key: "k".to_owned(),
        };
        let (stub_base, state) = stub_server(vec![
            StubReply {
                status: 200,
                headers: Vec::new(),
                bytes: Vec::new(),
                sleep_secs: 5,
            },
            StubReply {
                status: 200,
                headers: Vec::new(),
                bytes: Vec::new(),
                sleep_secs: 5,
            },
            StubReply {
                status: 200,
                headers: Vec::new(),
                bytes: Vec::new(),
                sleep_secs: 5,
            },
        ])
        .await;
        let request = StreamRequest {
            endpoint_url: format!("{stub_base}/v1/chat/completions"),
            body: encode_body(&serde_json::json!({"model": "m"})),
            timeout_secs: 1,
            extract: &table,
            lane: &lane,
        };
        let error = stream_turn(&client, &request, |_| {}).await.unwrap_err();
        assert!(
            matches!(&error, ProviderError::Timeout),
            "timeout surfaces: {error:?}"
        );
        assert_eq!(
            classify_provider_error(&lane, &error)
                .as_ref()
                .map(|(code, detail)| (code.as_str(), detail.as_str())),
            Some((
                "provider_unreachable",
                "Could not reach the configured provider endpoint."
            ))
        );
        assert_eq!(state.requests.lock().unwrap().len(), 3, "two retries");
    }

    /// The usage-limit streams (no stored blob) each parse to the
    /// same tool call with zero usage and a `tool_calls` finish.
    #[test]
    fn usage_limit_streams_parse() {
        let table = provider_table();
        let fixture = load_fixture("usage_limit");
        let exchanges = fixture
            .get("provider_exchanges")
            .and_then(Value::as_array)
            .unwrap();
        for (index, exchange) in exchanges.iter().enumerate() {
            let sse = exchange
                .get("response")
                .and_then(|r| r.get("sse_text"))
                .and_then(Value::as_str)
                .unwrap();
            let mut stream = OpenAIStream::new(&table, "http://stub/v1/", "oracle-stub");
            let events = stream.feed(sse.as_bytes()).unwrap();
            assert!(events.is_empty(), "usage_limit#{index} no text events");
            let turned = stream.finish().unwrap();
            assert_eq!(turned.parts.len(), 1, "usage_limit#{index} one part");
            let StreamPart::ToolCall(call) = &turned.parts[0] else {
                panic!("usage_limit#{index} streams a tool call");
            };
            assert_eq!(call.name, "list_projects");
            assert_eq!(call.call_id, "call_loop");
            assert!(matches!(&call.args, Some(ToolArgs::Raw(args)) if args == "{}"));
            assert_eq!(turned.usage, RequestUsage::zeros());
            assert!(matches!(turned.finish, Some(Finish::ToolCall)));
            assert_eq!(turned.model_name, "oracle-stub");
            assert_eq!(turned.response_id.as_deref(), Some("stub"));
            assert_eq!(
                turned.provider_details,
                Some(Map::from_iter([(
                    "finish_reason".to_owned(),
                    Value::String("tool_calls".to_owned())
                )]))
            );
        }
    }
}
