//! `model_messages` codec: pydantic-ai 1.107 `ModelMessage` JSON (PIDASHCONV-863).
//!
//! Ports the serialization half of `history.py` (`load_history`'s
//! `ModelMessagesTypeAdapter.validate_python` + `dump_new_messages`'s
//! `dump_python(..., mode='json')`). The SQL window (which turns to replay)
//! stays in `pidash_db::assistant::event_queries`; this module owns the
//! per-message schema: parsing stored blobs into the loop model, and
//! dumping new run messages back into the exact stored shape.
//!
//! Shape ground truth is `fixtures/assistant/turn-runtime.json`: two
//! in-process `Agent.run` captures against an OpenAI-compatible stub (a
//! text turn and a tool-call turn), plus the venv sources
//! (`pydantic_ai/messages.py`, `models/openai.py`). Every rule below
//! cites its probe:
//!
//! * Messages are `{kind: request|response, parts: [...]}`; parts carry
//!   a `part_kind` discriminator (`system-prompt`, `user-prompt`,
//!   `text`, `tool-call`, `tool-return`, `retry-prompt`, ...).
//! * `ToolCallPart.args` is the provider's raw arguments **string** on
//!   the OpenAI lane (spacing preserved verbatim) and an input
//!   **object** on the Anthropic lane — the dump keeps each lane's form.
//! * `tool_call_id` defaults via `generate_tool_call_id`
//!   (`messages.py:1122`, `pyd_ai_{uuid4hex}`) when absent.
//! * `finish_reason` normalizes through `_CHAT_FINISH_REASON_MAP`
//!   (`openai.py:254-264`): `stop`->`stop`, `length`->`length`,
//!   `tool_calls`/`function_call`->`tool_call`,
//!   `content_filter`->`content_filter`; a missing finish chunk stores
//!   null (the oracle stub sends none).
//! * Retry content is `str | list[ErrorDetails]`; the replay rendering
//!   is `RetryPromptPart.model_response` (`messages.py:1445-1467`).
//! * Dumped key order follows the captures (cosmetic: JSON objects are
//!   unordered, but matching order keeps eyeball diffs against Django
//!   blobs readable).
//!
//! Fidelity rule: anything `validate_python` would reject (unknown
//! `kind`/`part_kind`, wrong JSON types, non-object error details) is
//! a [`CodecError`] and the caller skips the turn with a warning —
//! exactly `history.py:45-52`'s `except Exception: continue`. Parts no
//! writer in this system emits (`system-prompt` parts, media content,
//! thinking/files/native tools) are rejected the same loud way rather
//! than silently dropped.

use pidash_jobs::oss_worker::model_runtime::provider::{generate_tool_call_id, ToolArgs, ToolCall};
use pidash_jobs::oss_worker::model_runtime::usage::RequestUsage;
use serde_json::{Map, Value};

/// A stored blob failed validation (`validate_python` raised): the
/// caller skips the turn, like `history.py:45-52`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecError(pub String);

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn err(message: impl Into<String>) -> CodecError {
    CodecError(message.into())
}

/// One retry error detail (`ErrorDetails`: `type`/`loc`/`msg`/`input`;
/// `url` and `ctx` are excluded at collection,
/// `error.errors(include_url=False, include_context=False)`).
#[derive(Debug, Clone, PartialEq)]
pub struct RetryErrorDetail {
    pub error_type: String,
    pub loc: Vec<RetryLocSeg>,
    pub msg: String,
    pub input: Value,
}

/// A location segment: object field or list index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryLocSeg {
    Field(String),
    Index(u64),
}

/// Retry feedback content (`RetryPromptPart.content`).
#[derive(Debug, Clone, PartialEq)]
pub enum TurnRetryContent {
    /// A `ModelRetry` message (unknown tool, or a tool's own retry).
    Text(String),
    /// Pydantic error details (tool-arg validation).
    Errors(Vec<RetryErrorDetail>),
}

/// A request-side part (the subset writers in this system emit).
#[derive(Debug, Clone, PartialEq)]
pub enum TurnRequestPart {
    /// `UserPromptPart(content: str)`.
    User(String),
    /// `ToolReturnPart`.
    ToolReturn {
        name: String,
        content: Value,
        call_id: String,
    },
    /// `RetryPromptPart`.
    Retry {
        tool_name: Option<String>,
        content: TurnRetryContent,
        call_id: String,
    },
}

/// A response-side part (`ToolCall` has no `PartialEq`, so neither does
/// this enum; tests match structurally instead).
#[derive(Debug, Clone)]
pub enum TurnResponsePart {
    Text(String),
    ToolCall(ToolCall),
}

/// Normalized finish reason (`FinishReason`, `messages.py:114-120`,
/// minus `error`: an error-finished response carries its partial parts
/// and the marker itself is never re-read, so it parses as `None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnFinish {
    Stop,
    Length,
    ToolCall,
    ContentFilter,
}

/// One conversation message (`ModelMessage`).
// The response arm is unavoidably larger (usage + metadata ride
// along); turns live in a `Vec` and move rarely.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum TurnMessage {
    Request {
        parts: Vec<TurnRequestPart>,
    },
    Response {
        parts: Vec<TurnResponsePart>,
        usage: RequestUsage,
        model_name: String,
        finish: Option<TurnFinish>,
        provider_details: Option<Map<String, Value>>,
        response_id: Option<String>,
    },
}

/// Parse one stored `model_messages` blob (`validate_python`).
/// The blob must be a JSON array of messages; anything else is a
/// [`CodecError`] and the caller skips the turn.
pub fn parse_blob(blob: &Value) -> Result<Vec<TurnMessage>, CodecError> {
    let items = blob
        .as_array()
        .ok_or_else(|| err("model_messages blob must be a JSON array"))?;
    items.iter().map(parse_message).collect()
}

fn parse_message(message: &Value) -> Result<TurnMessage, CodecError> {
    let object = message
        .as_object()
        .ok_or_else(|| err("history message must be an object"))?;
    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| err("history message needs a string kind"))?;
    let parts = object
        .get("parts")
        .and_then(Value::as_array)
        .ok_or_else(|| err("history message needs a parts array"))?;
    match kind {
        "request" => {
            let mut parsed = Vec::with_capacity(parts.len());
            for part in parts {
                parsed.push(parse_request_part(part)?);
            }
            Ok(TurnMessage::Request { parts: parsed })
        }
        "response" => {
            let mut parsed = Vec::with_capacity(parts.len());
            for part in parts {
                parsed.push(parse_response_part(part)?);
            }
            // Usage rides on the response but is never re-read: only the
            // current run's accumulator matters. Timestamps, run ids, and
            // provider names are likewise write-only metadata.
            let model_name = object
                .get("model_name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let finish = match object.get("finish_reason") {
                None | Some(Value::Null) => None,
                Some(Value::String(reason)) => parse_finish(reason)?,
                Some(_) => return Err(err("finish_reason must be a string or null")),
            };
            let provider_details = match object.get("provider_details") {
                None | Some(Value::Null) => None,
                Some(Value::Object(map)) => Some(map.clone()),
                Some(_) => return Err(err("provider_details must be an object or null")),
            };
            let response_id = match object.get("provider_response_id") {
                None | Some(Value::Null) => None,
                Some(Value::String(id)) => Some(id.clone()),
                Some(_) => return Err(err("provider_response_id must be a string or null")),
            };
            Ok(TurnMessage::Response {
                parts: parsed,
                usage: RequestUsage::zeros(),
                model_name,
                finish,
                provider_details,
                response_id,
            })
        }
        _ => Err(err(format!("unknown history message kind: {kind}"))),
    }
}

/// `FinishReason` (`messages.py:114-120`): `error` parses as `None`
/// (never re-read); anything else unknown is an error, like the
/// `Literal` validation.
fn parse_finish(reason: &str) -> Result<Option<TurnFinish>, CodecError> {
    match reason {
        "stop" => Ok(Some(TurnFinish::Stop)),
        "length" => Ok(Some(TurnFinish::Length)),
        "tool_call" => Ok(Some(TurnFinish::ToolCall)),
        "content_filter" => Ok(Some(TurnFinish::ContentFilter)),
        "error" => Ok(None),
        _ => Err(err(format!("unknown finish_reason: {reason}"))),
    }
}

fn part_object(part: &Value) -> Result<&Map<String, Value>, CodecError> {
    part.as_object()
        .ok_or_else(|| err("history part must be an object"))
}

fn part_kind(part: &Map<String, Value>) -> Result<&str, CodecError> {
    part.get("part_kind")
        .and_then(Value::as_str)
        .ok_or_else(|| err("history part needs a string part_kind"))
}

fn required_str(part: &Map<String, Value>, key: &str) -> Result<String, CodecError> {
    part.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| err(format!("history part needs a string {key}")))
}

/// `tool_call_id` (`messages.py:1122`): absent or null backfills with
/// a generated id, exactly the `default_factory`.
fn call_id_or_generated(part: &Map<String, Value>) -> Result<String, CodecError> {
    match part.get("tool_call_id") {
        None | Some(Value::Null) => Ok(generate_tool_call_id()),
        Some(Value::String(id)) => Ok(id.clone()),
        Some(_) => Err(err("tool_call_id must be a string or null")),
    }
}

fn parse_request_part(part: &Value) -> Result<TurnRequestPart, CodecError> {
    let object = part_object(part)?;
    match part_kind(object)? {
        "user-prompt" => match object.get("content") {
            Some(Value::String(text)) => Ok(TurnRequestPart::User(text.clone())),
            // Media content lists never occur in turns (the prompt is
            // the message row's plain text); reject loudly rather than
            // replaying a shape the wire mapping cannot render.
            _ => Err(err("user-prompt content must be a string")),
        },
        "tool-return" => Ok(TurnRequestPart::ToolReturn {
            name: required_str(object, "tool_name")?,
            content: object
                .get("content")
                .cloned()
                .ok_or_else(|| err("tool-return needs content"))?,
            call_id: call_id_or_generated(object)?,
        }),
        "retry-prompt" => {
            let content = match object.get("content") {
                Some(Value::String(text)) => TurnRetryContent::Text(text.clone()),
                Some(Value::Array(details)) => {
                    let mut parsed = Vec::with_capacity(details.len());
                    for detail in details {
                        parsed.push(parse_retry_detail(detail)?);
                    }
                    TurnRetryContent::Errors(parsed)
                }
                _ => return Err(err("retry-prompt content must be a string or error list")),
            };
            let tool_name = match object.get("tool_name") {
                None | Some(Value::Null) => None,
                Some(Value::String(name)) => Some(name.clone()),
                Some(_) => return Err(err("retry-prompt tool_name must be a string or null")),
            };
            Ok(TurnRequestPart::Retry {
                tool_name,
                content,
                call_id: call_id_or_generated(object)?,
            })
        }
        kind => Err(err(format!("unsupported request part_kind: {kind}"))),
    }
}

fn parse_response_part(part: &Value) -> Result<TurnResponsePart, CodecError> {
    let object = part_object(part)?;
    match part_kind(object)? {
        "text" => match object.get("content") {
            Some(Value::String(text)) => Ok(TurnResponsePart::Text(text.clone())),
            _ => Err(err("text part content must be a string")),
        },
        "tool-call" => {
            let args = match object.get("args") {
                Some(Value::String(raw)) => ToolArgs::Raw(raw.clone()),
                Some(Value::Object(map)) => ToolArgs::Dict(map.clone()),
                // The OpenAI lane stores the raw string, the Anthropic
                // lane the input object; anything else fails the
                // `str | dict` union validation.
                _ => return Err(err("tool-call args must be a string or object")),
            };
            Ok(TurnResponsePart::ToolCall(ToolCall {
                name: required_str(object, "tool_name")?,
                args,
                call_id: call_id_or_generated(object)?,
            }))
        }
        kind => Err(err(format!("unsupported response part_kind: {kind}"))),
    }
}

fn parse_retry_detail(detail: &Value) -> Result<RetryErrorDetail, CodecError> {
    let object = detail
        .as_object()
        .ok_or_else(|| err("retry error detail must be an object"))?;
    let error_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| err("retry error detail needs a string type"))?
        .to_owned();
    let loc = object
        .get("loc")
        .and_then(Value::as_array)
        .ok_or_else(|| err("retry error detail needs a loc array"))?
        .iter()
        .map(|seg| match seg {
            Value::String(field) => Ok(RetryLocSeg::Field(field.clone())),
            Value::Number(index) => index
                .as_u64()
                .map(RetryLocSeg::Index)
                .ok_or_else(|| err("retry error loc index must be a non-negative int")),
            _ => Err(err("retry error loc segments must be strings or ints")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let msg = object
        .get("msg")
        .and_then(Value::as_str)
        .ok_or_else(|| err("retry error detail needs a string msg"))?
        .to_owned();
    // `url`/`ctx` never survive collection; if a foreign blob carries
    // them they are ignored, exactly the exclusion rules.
    let input = object.get("input").cloned().unwrap_or(Value::Null);
    Ok(RetryErrorDetail {
        error_type,
        loc,
        msg,
        input,
    })
}

// ---------------------------------------------------------------------------
// Replay rendering
// ---------------------------------------------------------------------------

/// Render retry feedback for the wire (`RetryPromptPart.model_response`,
/// `messages.py:1445-1467`): str content reads verbatim with a tool
/// name, `Validation feedback:\n`-prefixed without one; error lists
/// render as `{n} validation error(s):\n```json\n{errors}\n``` ` with
/// the `input`/`ctx` exclusions, always suffixed with
/// `\n\nFix the errors and try again.`
pub fn retry_message(content: &TurnRetryContent, tool_name: Option<&str>) -> String {
    let description = match content {
        TurnRetryContent::Text(text) => {
            if tool_name.is_none() {
                format!("Validation feedback:\n{text}")
            } else {
                text.clone()
            }
        }
        TurnRetryContent::Errors(details) => {
            let errors: Vec<Value> = details
                .iter()
                .map(|detail| render_retry_error(detail, tool_name.is_none()))
                .collect();
            let pretty =
                serde_json::to_string_pretty(&Value::Array(errors)).unwrap_or("[]".to_owned());
            let body = ensure_ascii(&pretty);
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

/// One retry error object (the `model_response` exclusions): without a
/// tool name, top-level errors (loc of length <= 1) drop `ctx` and
/// `input`; deeper ones drop `ctx`. With a tool name every error drops
/// `ctx` only. `url` never survives collection.
fn render_retry_error(detail: &RetryErrorDetail, no_tool: bool) -> Value {
    let mut error = Map::new();
    error.insert("type".to_owned(), Value::String(detail.error_type.clone()));
    error.insert(
        "loc".to_owned(),
        Value::Array(
            detail
                .loc
                .iter()
                .map(|seg| match seg {
                    RetryLocSeg::Field(name) => Value::String(name.clone()),
                    RetryLocSeg::Index(index) => Value::Number((*index).into()),
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

/// `json.dumps(..., ensure_ascii=True)`: every `c >= 0x7F` inside a
/// string becomes `\uXXXX` (lowercase hex; surrogate pairs for astral
/// planes). String boundaries are tracked with escape awareness;
/// structural characters are never touched.
fn ensure_ascii(pretty: &str) -> String {
    let mut out = String::with_capacity(pretty.len());
    let mut chars = pretty.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    for c in chars.by_ref() {
        if !in_string {
            out.push(c);
            if c == '"' {
                in_string = true;
            }
            continue;
        }
        if escaped {
            out.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' => {
                out.push(c);
                escaped = true;
            }
            '"' => {
                out.push(c);
                in_string = false;
            }
            c if (c as u32) < 0x7f => out.push(c),
            c => {
                let code = c as u32;
                if code <= 0xffff {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    let shifted = code - 0x1_0000;
                    let high = 0xd800 + (shifted >> 10);
                    let low = 0xdc00 + (shifted & 0x3ff);
                    out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Dump
// ---------------------------------------------------------------------------

/// Fresh run identifiers, stamped on every new message of one run
/// (both uuid7, constant within the run, fresh across runs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunIds {
    pub run_id: String,
    pub conversation_id: String,
}

impl RunIds {
    pub fn fresh() -> Self {
        RunIds {
            run_id: uuid::Uuid::now_v7().to_string(),
            conversation_id: uuid::Uuid::now_v7().to_string(),
        }
    }
}

/// `datetime.now(timezone.utc)` rendering (`dump_python(mode='json')`):
/// microseconds + `Z`.
pub fn now_stamp() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.6fZ")
        .to_string()
}

/// Dump one new request message (`dump_python`, request arm). Key order
/// follows the captures: parts, timestamp, instructions, kind, run_id,
/// conversation_id, metadata.
pub fn dump_request(
    instructions: &str,
    parts: &[TurnRequestPart],
    ids: &RunIds,
    timestamp: &str,
) -> Value {
    let dumped: Vec<Value> = parts
        .iter()
        .map(|part| dump_request_part(part, timestamp))
        .collect();
    Value::Object(Map::from_iter([
        ("parts".to_owned(), Value::Array(dumped)),
        ("timestamp".to_owned(), Value::String(timestamp.to_owned())),
        (
            "instructions".to_owned(),
            Value::String(instructions.to_owned()),
        ),
        ("kind".to_owned(), Value::String("request".to_owned())),
        ("run_id".to_owned(), Value::String(ids.run_id.clone())),
        (
            "conversation_id".to_owned(),
            Value::String(ids.conversation_id.clone()),
        ),
        ("metadata".to_owned(), Value::Null),
    ]))
}

fn dump_request_part(part: &TurnRequestPart, timestamp: &str) -> Value {
    match part {
        TurnRequestPart::User(text) => Value::Object(Map::from_iter([
            ("content".to_owned(), Value::String(text.clone())),
            ("timestamp".to_owned(), Value::String(timestamp.to_owned())),
            (
                "part_kind".to_owned(),
                Value::String("user-prompt".to_owned()),
            ),
        ])),
        TurnRequestPart::ToolReturn {
            name,
            content,
            call_id,
        } => Value::Object(Map::from_iter([
            ("tool_name".to_owned(), Value::String(name.clone())),
            ("content".to_owned(), content.clone()),
            ("tool_call_id".to_owned(), Value::String(call_id.clone())),
            ("tool_kind".to_owned(), Value::Null),
            ("metadata".to_owned(), Value::Null),
            ("timestamp".to_owned(), Value::String(timestamp.to_owned())),
            ("outcome".to_owned(), Value::String("success".to_owned())),
            (
                "part_kind".to_owned(),
                Value::String("tool-return".to_owned()),
            ),
        ])),
        TurnRequestPart::Retry {
            tool_name,
            content,
            call_id,
        } => Value::Object(Map::from_iter([
            ("content".to_owned(), dump_retry_content(content)),
            (
                "tool_name".to_owned(),
                tool_name
                    .as_ref()
                    .map(|name| Value::String(name.clone()))
                    .unwrap_or(Value::Null),
            ),
            ("tool_call_id".to_owned(), Value::String(call_id.clone())),
            ("timestamp".to_owned(), Value::String(timestamp.to_owned())),
            (
                "part_kind".to_owned(),
                Value::String("retry-prompt".to_owned()),
            ),
        ])),
    }
}

/// Retry content dumps in stored form: text verbatim, error details as
/// the `{type, loc, msg, input}` list (the `model_response` rendering
/// happens at request time, never in storage).
fn dump_retry_content(content: &TurnRetryContent) -> Value {
    match content {
        TurnRetryContent::Text(text) => Value::String(text.clone()),
        TurnRetryContent::Errors(details) => Value::Array(
            details
                .iter()
                .map(|detail| {
                    Value::Object(Map::from_iter([
                        ("type".to_owned(), Value::String(detail.error_type.clone())),
                        (
                            "loc".to_owned(),
                            Value::Array(
                                detail
                                    .loc
                                    .iter()
                                    .map(|seg| match seg {
                                        RetryLocSeg::Field(name) => Value::String(name.clone()),
                                        RetryLocSeg::Index(index) => Value::Number((*index).into()),
                                    })
                                    .collect(),
                            ),
                        ),
                        ("msg".to_owned(), Value::String(detail.msg.clone())),
                        ("input".to_owned(), detail.input.clone()),
                    ]))
                })
                .collect(),
        ),
    }
}

/// Dump one new response message (`dump_python`, response arm). Key
/// order follows the captures.
#[allow(clippy::too_many_arguments)]
pub fn dump_response(
    parts: &[TurnResponsePart],
    usage: &RequestUsage,
    model_name: &str,
    finish: Option<TurnFinish>,
    provider_details: Option<&Map<String, Value>>,
    response_id: Option<&str>,
    provider_name: &str,
    provider_url: &str,
    ids: &RunIds,
    timestamp: &str,
) -> Value {
    let dumped: Vec<Value> = parts.iter().map(dump_response_part).collect();
    let finish_reason = finish.map(|finish| match finish {
        TurnFinish::Stop => "stop",
        TurnFinish::Length => "length",
        TurnFinish::ToolCall => "tool_call",
        TurnFinish::ContentFilter => "content_filter",
    });
    Value::Object(Map::from_iter([
        ("parts".to_owned(), Value::Array(dumped)),
        ("usage".to_owned(), dump_usage(usage)),
        (
            "model_name".to_owned(),
            Value::String(model_name.to_owned()),
        ),
        ("timestamp".to_owned(), Value::String(timestamp.to_owned())),
        ("kind".to_owned(), Value::String("response".to_owned())),
        (
            "provider_name".to_owned(),
            Value::String(provider_name.to_owned()),
        ),
        (
            "provider_url".to_owned(),
            Value::String(provider_url.to_owned()),
        ),
        (
            "provider_details".to_owned(),
            provider_details
                .map(|details| Value::Object(details.clone()))
                .unwrap_or(Value::Null),
        ),
        (
            "provider_response_id".to_owned(),
            response_id
                .map(|id| Value::String(id.to_owned()))
                .unwrap_or(Value::Null),
        ),
        (
            "finish_reason".to_owned(),
            finish_reason
                .map(|reason| Value::String(reason.to_owned()))
                .unwrap_or(Value::Null),
        ),
        ("run_id".to_owned(), Value::String(ids.run_id.clone())),
        (
            "conversation_id".to_owned(),
            Value::String(ids.conversation_id.clone()),
        ),
        ("metadata".to_owned(), Value::Null),
        ("state".to_owned(), Value::String("complete".to_owned())),
    ]))
}

fn dump_response_part(part: &TurnResponsePart) -> Value {
    match part {
        TurnResponsePart::Text(text) => Value::Object(Map::from_iter([
            ("content".to_owned(), Value::String(text.clone())),
            ("id".to_owned(), Value::Null),
            ("provider_name".to_owned(), Value::Null),
            ("provider_details".to_owned(), Value::Null),
            ("part_kind".to_owned(), Value::String("text".to_owned())),
        ])),
        TurnResponsePart::ToolCall(call) => {
            // Each lane's stored form: the OpenAI raw arguments string
            // verbatim (provider spacing intact), the Anthropic input
            // object as-is.
            let args = match &call.args {
                ToolArgs::Raw(raw) => Value::String(raw.clone()),
                ToolArgs::Dict(map) => Value::Object(map.clone()),
            };
            Value::Object(Map::from_iter([
                ("tool_name".to_owned(), Value::String(call.name.clone())),
                ("args".to_owned(), args),
                (
                    "tool_call_id".to_owned(),
                    Value::String(call.call_id.clone()),
                ),
                ("tool_kind".to_owned(), Value::Null),
                ("id".to_owned(), Value::Null),
                ("provider_name".to_owned(), Value::Null),
                ("provider_details".to_owned(), Value::Null),
                (
                    "part_kind".to_owned(),
                    Value::String("tool-call".to_owned()),
                ),
            ]))
        }
    }
}

fn dump_usage(usage: &RequestUsage) -> Value {
    Value::Object(Map::from_iter([
        ("input_tokens".to_owned(), usage.input_tokens.to_json()),
        (
            "cache_write_tokens".to_owned(),
            usage.cache_write_tokens.to_json(),
        ),
        (
            "cache_read_tokens".to_owned(),
            usage.cache_read_tokens.to_json(),
        ),
        ("output_tokens".to_owned(), usage.output_tokens.to_json()),
        (
            "input_audio_tokens".to_owned(),
            usage.input_audio_tokens.to_json(),
        ),
        (
            "cache_audio_read_tokens".to_owned(),
            usage.cache_audio_read_tokens.to_json(),
        ),
        (
            "output_audio_tokens".to_owned(),
            usage.output_audio_tokens.to_json(),
        ),
        ("details".to_owned(), Value::Object(usage.details.clone())),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/assistant/turn-runtime.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    /// Volatile fields (timestamps, run ids, stub port, provider chunk
    /// ids) differ on every run; everything else must match exactly.
    fn normalize(value: &Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(key, val)| {
                        let normalized = match key.as_str() {
                            "timestamp" | "run_id" | "conversation_id" | "provider_response_id" => {
                                Value::String("*".to_owned())
                            }
                            "provider_url" => Value::String("*".to_owned()),
                            "tool_call_id" if val.is_null() => val.clone(),
                            _ => normalize(val),
                        };
                        (key.clone(), normalized)
                    })
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.iter().map(normalize).collect()),
            _ => value.clone(),
        }
    }

    #[test]
    fn parse_text_turn_blob() {
        let blob = &fixture()["text_turn"]["new_messages"];
        let messages = parse_blob(blob).expect("text blob parses");
        assert_eq!(messages.len(), 2);
        assert!(matches!(
            messages[0],
            TurnMessage::Request { ref parts }
            if matches!(parts.as_slice(), [TurnRequestPart::User(text)] if text == "do a thing")
        ));
        match &messages[1] {
            TurnMessage::Response {
                parts,
                model_name,
                finish,
                provider_details,
                response_id,
                ..
            } => {
                assert!(
                    matches!(parts.as_slice(), [TurnResponsePart::Text(text)] if text == "Hello world")
                );
                assert_eq!(model_name, "oracle-stub");
                // The oracle stub sends no finish chunk: null stays null.
                assert_eq!(*finish, None);
                assert_eq!(*provider_details, None);
                assert_eq!(response_id.as_deref(), Some("stub"));
            }
            TurnMessage::Request { .. } => panic!("second message must be a response"),
        }
    }

    #[test]
    fn parse_tool_turn_blob() {
        let blob = &fixture()["tool_turn"]["new_messages"];
        let messages = parse_blob(blob).expect("tool blob parses");
        assert_eq!(messages.len(), 4);
        // The tool call keeps the provider's raw arguments string
        // verbatim (spacing intact), and the normalized finish reason.
        match &messages[1] {
            TurnMessage::Response { parts, finish, .. } => {
                assert_eq!(*finish, Some(TurnFinish::ToolCall));
                assert!(
                    matches!(parts.as_slice(), [TurnResponsePart::ToolCall(call)]
                        if call.name == "get_issue"
                            && matches!(&call.args, ToolArgs::Raw(raw) if raw == "{\"issue\": \"ENG-1\"}")
                            && call.call_id == "call_1")
                );
            }
            _ => panic!("second message must be the tool-call response"),
        }
        // The tool result keeps its structured content.
        assert!(matches!(
            messages[2],
            TurnMessage::Request { ref parts }
            if matches!(parts.as_slice(), [TurnRequestPart::ToolReturn { ref name, ref content, ref call_id }]
                if name == "get_issue" && content == &json!({"identifier": "ENG-1", "title": "T"}) && call_id == "call_1")
        ));
        match &messages[3] {
            TurnMessage::Response { parts, finish, .. } => {
                assert_eq!(*finish, Some(TurnFinish::Stop));
                assert!(
                    matches!(parts.as_slice(), [TurnResponsePart::Text(text)] if text == "Issue ENG-1.")
                );
            }
            _ => panic!("fourth message must be the text response"),
        }
    }

    #[test]
    fn dump_round_trip_matches_capture_modulo_volatile() {
        for turn in ["text_turn", "tool_turn"] {
            let blob = &fixture()[turn]["new_messages"];
            let messages = parse_blob(blob).expect("blob parses");
            let ids = RunIds {
                run_id: "run".to_owned(),
                conversation_id: "conv".to_owned(),
            };
            let mut dumped = Vec::new();
            for message in &messages {
                match message {
                    TurnMessage::Request { parts } => {
                        dumped.push(dump_request("You are a test.", parts, &ids, "ts"));
                    }
                    TurnMessage::Response {
                        parts,
                        usage,
                        model_name,
                        finish,
                        provider_details,
                        response_id,
                        ..
                    } => dumped.push(dump_response(
                        parts,
                        usage,
                        model_name,
                        *finish,
                        provider_details.as_ref(),
                        response_id.as_deref(),
                        "openai",
                        "http://stub/v1/",
                        &ids,
                        "ts",
                    )),
                }
            }
            // The re-dump must carry the same key sets, part kinds, and
            // content as the capture; volatile fields normalize away.
            // Instructions are run inputs (the capture's "You are a
            // test."), model/provider stamps come from the lane.
            let expected = normalize(blob);
            let actual = normalize(&Value::Array(dumped));
            assert_eq!(actual, expected, "{turn} re-dump must match capture");
        }
    }

    #[test]
    fn rejects_unknown_kinds_and_shapes() {
        // Unknown message kind.
        assert!(parse_blob(&json!([{"kind": "event", "parts": []}])).is_err());
        // Non-array blob.
        assert!(parse_blob(&json!({"kind": "request"})).is_err());
        // Unknown part kinds fail the blob (skip the turn), like
        // `validate_python` raising.
        assert!(parse_blob(&json!([{
            "kind": "request",
            "parts": [{"part_kind": "teleport", "content": "x"}],
        }]))
        .is_err());
        assert!(parse_blob(&json!([{
            "kind": "response",
            "parts": [{"part_kind": "thinking", "content": "hmm"}],
        }]))
        .is_err());
        // System-prompt parts: no writer emits them; reject loudly.
        assert!(parse_blob(&json!([{
            "kind": "request",
            "parts": [{"part_kind": "system-prompt", "content": "sys"}],
        }]))
        .is_err());
        // Media user content: turns only replay plain text.
        assert!(parse_blob(&json!([{
            "kind": "request",
            "parts": [{"part_kind": "user-prompt", "content": [{"text": "x"}]}],
        }]))
        .is_err());
        // Wrong JSON types.
        assert!(parse_blob(&json!([{
            "kind": "request",
            "parts": [{"part_kind": "user-prompt"}],
        }]))
        .is_err());
        assert!(parse_blob(&json!([{
            "kind": "response",
            "parts": [{"part_kind": "text", "content": "x"}],
            "finish_reason": "warp",
        }]))
        .is_err());
        // Missing tool_call_id backfills (the default_factory).
        let messages = parse_blob(&json!([{
            "kind": "response",
            "parts": [{"part_kind": "tool-call", "tool_name": "t", "args": "{}"}],
        }]))
        .expect("missing id backfills");
        assert!(matches!(
            messages.as_slice(),
            [TurnMessage::Response { parts, .. }]
            if matches!(parts.as_slice(), [TurnResponsePart::ToolCall(call)]
                if call.call_id.starts_with("pyd_ai_"))
        ));
        // `error` finish parses as None (never re-read).
        let messages = parse_blob(&json!([{
            "kind": "response",
            "parts": [],
            "finish_reason": "error",
        }]))
        .expect("error finish parses");
        assert!(matches!(
            messages.as_slice(),
            [TurnMessage::Response { finish: None, .. }]
        ));
    }

    #[test]
    fn retry_errors_render_like_model_response() {
        let content = TurnRetryContent::Errors(vec![RetryErrorDetail {
            error_type: "missing".to_owned(),
            loc: vec![RetryLocSeg::Field("issue".to_owned())],
            msg: "Field required".to_owned(),
            input: json!({"other": 1}),
        }]);
        // With a tool name: the body keeps `input`, drops `ctx`.
        let rendered = retry_message(&content, Some("get_issue"));
        assert!(rendered.contains("\"type\": \"missing\""), "{rendered}");
        assert!(rendered.contains("\"input\""), "{rendered}");
        assert!(!rendered.contains("\"ctx\""), "{rendered}");
        assert!(rendered.ends_with("\n\nFix the errors and try again."));
        assert!(rendered.starts_with("1 validation error:\n```json\n"));
        // Without a tool name a top-level error drops `input` too, and
        // a text body gains the `Validation feedback:` prefix.
        let rendered = retry_message(&content, None);
        assert!(!rendered.contains("\"input\""), "{rendered}");
        let text = retry_message(&TurnRetryContent::Text("nope".to_owned()), None);
        assert_eq!(
            text,
            "Validation feedback:\nnope\n\nFix the errors and try again."
        );
        let text = retry_message(&TurnRetryContent::Text("nope".to_owned()), Some("t"));
        assert_eq!(text, "nope\n\nFix the errors and try again.");
    }

    #[test]
    fn retry_errors_dump_as_detail_list() {
        let part = TurnRequestPart::Retry {
            tool_name: Some("get_issue".to_owned()),
            content: TurnRetryContent::Errors(vec![RetryErrorDetail {
                error_type: "missing".to_owned(),
                loc: vec![RetryLocSeg::Field("issue".to_owned())],
                msg: "Field required".to_owned(),
                input: json!(7),
            }]),
            call_id: "call_9".to_owned(),
        };
        let ids = RunIds {
            run_id: "r".to_owned(),
            conversation_id: "c".to_owned(),
        };
        let dumped = dump_request("instr", std::slice::from_ref(&part), &ids, "ts");
        let content = &dumped["parts"][0]["content"];
        assert_eq!(
            content,
            &json!([{"type": "missing", "loc": ["issue"], "msg": "Field required", "input": 7}]),
        );
        // The stored list parses back into the same content.
        let back = parse_blob(&json!([{
            "kind": "request",
            "parts": [{
                "part_kind": "retry-prompt",
                "content": content,
                "tool_name": "get_issue",
                "tool_call_id": "call_9",
            }],
        }]))
        .expect("dumped retry parses");
        assert!(matches!(
            back.as_slice(),
            [TurnMessage::Request { parts }] if parts == std::slice::from_ref(&part)
        ));
    }

    #[test]
    fn dump_key_order_follows_capture() {
        let ids = RunIds {
            run_id: "r".to_owned(),
            conversation_id: "c".to_owned(),
        };
        let request = dump_request(
            "instr",
            &[TurnRequestPart::User("hi".to_owned())],
            &ids,
            "ts",
        );
        let keys: Vec<&str> = request
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "parts",
                "timestamp",
                "instructions",
                "kind",
                "run_id",
                "conversation_id",
                "metadata"
            ]
        );
        let response = dump_response(
            &[TurnResponsePart::Text("hi".to_owned())],
            &RequestUsage::zeros(),
            "m",
            Some(TurnFinish::Stop),
            None,
            None,
            "openai",
            "http://stub/v1/",
            &ids,
            "ts",
        );
        let keys: Vec<&str> = response
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "parts",
                "usage",
                "model_name",
                "timestamp",
                "kind",
                "provider_name",
                "provider_url",
                "provider_details",
                "provider_response_id",
                "finish_reason",
                "run_id",
                "conversation_id",
                "metadata",
                "state"
            ]
        );
    }

    #[test]
    fn stamps_and_ids_have_capture_shape() {
        let stamp = now_stamp();
        // `%Y-%m-%dT%H:%M:%S%.6fZ`, e.g. `2026-10-17T06:19:46.080018Z`.
        assert_eq!(stamp.len(), 27, "{stamp}");
        assert!(stamp.ends_with('Z'));
        let ids = RunIds::fresh();
        assert_ne!(ids.run_id, ids.conversation_id);
        // uuid7 (version nibble `7`).
        assert_eq!(ids.run_id.chars().nth(14), Some('7'));
    }
}
