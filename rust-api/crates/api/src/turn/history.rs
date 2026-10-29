//! `model_messages` codec: pydantic-ai 1.107 `ModelMessage` JSON (PIDASHCONV-1110).
//!
//! Ports the serialization half of `history.py` (`load_history`'s
//! `ModelMessagesTypeAdapter.validate_python` + `dump_new_messages`'s
//! `dump_python(..., mode='json')`). The SQL window (which turns to replay)
//! stays in `pidash_db::assistant::event_queries`; this module owns the
//! per-message schema: parsing stored blobs into the loop model, and
//! dumping new run messages back into the exact stored shape.
//!
//! Shape ground truth is `fixtures/assistant/turn-runtime/*.json`: ten
//! real `_run_turn` captures against scripted OpenAI/Anthropic/MCP stubs
//! (text turns on both lanes, local/MCP/Anthropic tool round-trips, a
//! validation-retry turn, a 3-turn replay, 402/401 failures, a
//! usage-limit stop), plus the venv sources (`pydantic_ai/messages.py`,
//! `models/openai.py`, `models/anthropic.py`). Every rule below cites
//! its probe:
//!
//! * Messages are `{kind: request|response, parts: [...]}`; parts carry
//!   a `part_kind` discriminator (`system-prompt`, `user-prompt`,
//!   `text`, `tool-call`, `tool-return`, `retry-prompt`, ...).
//! * `ToolCallPart.args` is `str | dict | None`: streamed turns store
//!   the raw arguments **string** on both lanes (the Anthropic
//!   `input_json` deltas accumulate as a string); the object form
//!   appears on the non-streaming path (`anthropic.py:944`); null
//!   survives when a streamed `tool_use` block yields zero input
//!   deltas (`anthropic.py:2222`). The dump keeps each form verbatim.
//! * `tool_call_id` defaults via `generate_tool_call_id`
//!   (`_utils.py:431-436`, `pyd_ai_{uuid4hex}`) when the key is absent;
//!   an explicit null fails validation, like the `str` field.
//! * Stored blobs read back from the turn table are in Postgres jsonb
//!   key order (length, then bytewise) — the captures pin that order,
//!   and the round-trip test compares at the JSON-value level. The
//!   dump emits `dump_python` field order (probe-pinned
//!   `.probe1110-dumporder.py`), which is what Django hands the driver;
//!   Postgres normalizes both identically on write.
//! * Retry content is `str | list[ErrorDetails]` with `input` required
//!   (probed: `validate_python` rejects a detail without it); the
//!   replay rendering is `RetryPromptPart.model_response`
//!   (`messages.py:1445-1467`). `url`/`ctx` never survive collection.
//! * `provider_details`/`provider_response_id` also accept the
//!   `vendor_details`/`vendor_id` aliases (`ModelResponse` field
//!   aliases); the dump emits the canonical names.
//! * `outcome` is `success|failed|denied` (default `success`), `state`
//!   is `complete|incomplete|interrupted` (default `complete`),
//!   `finish_reason` is the five-valued `FinishReason` literal
//!   including `error`.
//!
//! Fidelity rule: anything `validate_python` would reject (unknown
//! `kind`/`part_kind`, wrong JSON types, a retry detail without
//! `input`) is a [`CodecError`] and the caller skips the turn with a
//! warning — exactly `history.py:45-52`'s `except Exception: continue`.
//! A few shapes `validate_python` accepts are rejected the same loud
//! way because no writer in this system emits them and the loop could
//! not execute them faithfully: media user content (the prompt is the
//! message row's plain text), `system-prompt` parts (instructions ride
//! on the message, never as parts), thinking/file/audio/native parts,
//! and non-null `tool_kind` (typed call/return subclasses whose extra
//! fields the model cannot carry — accepting them would silently drop
//! fields on re-dump). A loud skip beats a silent corruption.

use pidash_jobs::oss_worker::model_runtime::provider::{generate_tool_call_id, ToolArgs};
use pidash_jobs::oss_worker::model_runtime::usage::{RequestUsage, UsageNumber};
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

/// One retry error detail (`ErrorDetails`: `type`/`loc`/`msg`/`input`,
/// all required — probed; `url` and `ctx` are excluded at collection,
/// `error.errors(include_url=False, include_context=False)`).
#[derive(Debug, Clone, PartialEq)]
pub struct RetryErrorDetail {
    pub error_type: String,
    pub loc: Vec<RetryLocSeg>,
    pub msg: String,
    pub input: Value,
}

/// A location segment: object field or list index (`int`, signed
/// like the `loc` tuple — real validation errors only carry
/// non-negative indices, but the codec stays no stricter than
/// `validate_python`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryLocSeg {
    Field(String),
    Index(i64),
}

/// Retry feedback content (`RetryPromptPart.content`).
#[derive(Debug, Clone, PartialEq)]
pub enum TurnRetryContent {
    /// A `ModelRetry` message (unknown tool, or a tool's own retry).
    Text(String),
    /// Pydantic error details (tool-arg validation).
    Errors(Vec<RetryErrorDetail>),
}

/// Tool-result outcome (`ToolReturnPart.outcome`,
/// `success|failed|denied`, default `success`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnOutcome {
    Success,
    Failed,
    Denied,
}

/// A request-side part (plus the stored stamps round-tripping with it).
#[derive(Debug, Clone, PartialEq)]
pub enum TurnRequestPart {
    /// `UserPromptPart(content: str)`.
    User {
        text: String,
        timestamp: Option<String>,
    },
    /// `ToolReturnPart`.
    ToolReturn {
        name: String,
        content: Value,
        call_id: String,
        timestamp: Option<String>,
        metadata: Value,
        outcome: TurnOutcome,
    },
    /// `RetryPromptPart`.
    Retry {
        tool_name: Option<String>,
        content: TurnRetryContent,
        call_id: String,
        timestamp: Option<String>,
    },
}

/// A response-side part (tests match structurally instead of
/// deriving `PartialEq`).
#[derive(Debug, Clone)]
pub enum TurnResponsePart {
    Text {
        text: String,
        id: Option<String>,
        provider_name: Option<String>,
        provider_details: Option<Map<String, Value>>,
    },
    ToolCall {
        name: String,
        /// `None` is a stored null: `args` defaults to `None`
        /// (`messages.py:1776`), and the Anthropic lane stores null when
        /// a `tool_use` block yields zero `input_json` deltas
        /// (`anthropic.py:2222`, `... or None`, with no block-stop
        /// normalization for the regular path). The loop reads `None`
        /// as `{}` (`args_as_dict`); the dump reproduces the null.
        args: Option<ToolArgs>,
        call_id: String,
        id: Option<String>,
        provider_name: Option<String>,
        provider_details: Option<Map<String, Value>>,
    },
}

/// Normalized finish reason (`FinishReason`, `messages.py:117-123`).
/// `error` is preserved as a variant: an error-finished response
/// carries its partial parts, and the dump reproduces the marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnFinish {
    Stop,
    Length,
    ToolCall,
    ContentFilter,
    Error,
}

/// Response lifecycle state (`ModelResponseState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnState {
    Complete,
    Incomplete,
    Interrupted,
}

/// One conversation message (`ModelMessage`), with every stored field
/// preserved so `dump(parse(blob))` reproduces the blob.
#[derive(Debug, Clone)]
// The response arm is unavoidably larger (usage + metadata ride
// along); turns live in a `Vec` and move rarely.
#[allow(clippy::large_enum_variant)]
pub enum TurnMessage {
    Request {
        parts: Vec<TurnRequestPart>,
        timestamp: Option<String>,
        instructions: Option<String>,
        run_id: Option<String>,
        conversation_id: Option<String>,
        metadata: Value,
    },
    Response {
        parts: Vec<TurnResponsePart>,
        usage: RequestUsage,
        model_name: Option<String>,
        timestamp: String,
        provider_name: Option<String>,
        provider_url: Option<String>,
        provider_details: Option<Map<String, Value>>,
        response_id: Option<String>,
        finish: Option<TurnFinish>,
        run_id: Option<String>,
        conversation_id: Option<String>,
        metadata: Value,
        state: TurnState,
    },
}

impl TurnMessage {
    /// Build a new request message (fresh run stamps supplied by the
    /// caller); [`dump_message`] serializes it into the stored shape.
    pub fn new_request(
        parts: Vec<TurnRequestPart>,
        instructions: Option<String>,
        ids: &RunIds,
        timestamp: &str,
    ) -> Self {
        TurnMessage::Request {
            parts,
            timestamp: Some(timestamp.to_owned()),
            instructions,
            run_id: Some(ids.run_id.clone()),
            conversation_id: Some(ids.conversation_id.clone()),
            metadata: Value::Null,
        }
    }

    /// Build a new response message (fresh run stamps supplied by the
    /// caller); [`dump_message`] serializes it into the stored shape.
    #[allow(clippy::too_many_arguments)]
    pub fn new_response(
        parts: Vec<TurnResponsePart>,
        usage: RequestUsage,
        model_name: Option<String>,
        timestamp: &str,
        provider_name: Option<String>,
        provider_url: Option<String>,
        provider_details: Option<Map<String, Value>>,
        response_id: Option<String>,
        finish: Option<TurnFinish>,
        ids: &RunIds,
    ) -> Self {
        TurnMessage::Response {
            parts,
            usage,
            model_name,
            timestamp: timestamp.to_owned(),
            provider_name,
            provider_url,
            provider_details,
            response_id,
            finish,
            run_id: Some(ids.run_id.clone()),
            conversation_id: Some(ids.conversation_id.clone()),
            metadata: Value::Null,
            state: TurnState::Complete,
        }
    }
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

/// Python truthiness for a stored blob (`history.py:46`, `if not blob`).
fn is_falsy_blob(blob: &Value) -> bool {
    match blob {
        Value::Null => true,
        Value::Bool(flag) => !flag,
        Value::Number(number) => number.as_f64() == Some(0.0),
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
    }
}

/// Join per-turn blobs for replay (`history.py:45-52`): falsy blobs are
/// skipped, each remaining blob is validated on its own, and a blob that
/// fails is skipped with a warning — one undecodable stored turn never
/// fails the turns replaying after it. The joined list holds the raw
/// items of the blobs that parsed only, so a later single-pass parse of
/// the joined list cannot fail on a skipped turn.
pub fn join_valid_blobs(blobs: &[Value]) -> Vec<Value> {
    let mut messages = Vec::new();
    for blob in blobs {
        if is_falsy_blob(blob) {
            continue;
        }
        match parse_blob(blob) {
            Ok(_) => {
                if let Value::Array(items) = blob {
                    messages.extend(items.iter().cloned());
                }
            }
            Err(error) => {
                tracing::warn!(
                    "assistant: could not deserialize a stored turn's history; skipping it: {error}"
                );
            }
        }
    }
    messages
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
            Ok(TurnMessage::Request {
                parts: parsed,
                timestamp: opt_str(object, "timestamp")?,
                instructions: opt_str(object, "instructions")?,
                run_id: opt_str(object, "run_id")?,
                conversation_id: opt_str(object, "conversation_id")?,
                metadata: opt_metadata(object)?,
            })
        }
        "response" => {
            let mut parsed = Vec::with_capacity(parts.len());
            for part in parts {
                parsed.push(parse_response_part(part)?);
            }
            Ok(TurnMessage::Response {
                parts: parsed,
                usage: parse_usage(object)?,
                model_name: opt_str(object, "model_name")?,
                // `timestamp: datetime` (auto default): the writer always
                // emits it, and a missing stamp has no faithful re-dump.
                timestamp: required_str(object, "timestamp")?,
                provider_name: opt_str(object, "provider_name")?,
                provider_url: opt_str(object, "provider_url")?,
                provider_details: opt_details(object, "provider_details", "vendor_details")?,
                response_id: opt_str_alias(object, "provider_response_id", "vendor_id")?,
                finish: opt_finish(object)?,
                run_id: opt_str(object, "run_id")?,
                conversation_id: opt_str(object, "conversation_id")?,
                metadata: opt_metadata(object)?,
                state: parse_state(object)?,
            })
        }
        _ => Err(err(format!("unknown history message kind: {kind}"))),
    }
}

/// `FinishReason` (`messages.py:117-123`): all five literals parse;
/// anything else unknown is an error, like the `Literal` validation.
fn parse_finish_value(reason: &str) -> Result<TurnFinish, CodecError> {
    match reason {
        "stop" => Ok(TurnFinish::Stop),
        "length" => Ok(TurnFinish::Length),
        "tool_call" => Ok(TurnFinish::ToolCall),
        "content_filter" => Ok(TurnFinish::ContentFilter),
        "error" => Ok(TurnFinish::Error),
        _ => Err(err(format!("unknown finish_reason: {reason}"))),
    }
}

fn opt_finish(object: &Map<String, Value>) -> Result<Option<TurnFinish>, CodecError> {
    match object.get("finish_reason") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(reason)) => parse_finish_value(reason).map(Some),
        Some(_) => Err(err("finish_reason must be a string or null")),
    }
}

/// `ModelResponseState` (default `complete` when absent).
fn parse_state(object: &Map<String, Value>) -> Result<TurnState, CodecError> {
    match object.get("state") {
        None => Ok(TurnState::Complete),
        Some(Value::String(state)) => match state.as_str() {
            "complete" => Ok(TurnState::Complete),
            "incomplete" => Ok(TurnState::Incomplete),
            "interrupted" => Ok(TurnState::Interrupted),
            _ => Err(err(format!("unknown response state: {state}"))),
        },
        Some(_) => Err(err("state must be a string")),
    }
}

/// `ToolReturnPart.outcome` (default `success` when absent).
fn parse_outcome(part: &Map<String, Value>) -> Result<TurnOutcome, CodecError> {
    match part.get("outcome") {
        None => Ok(TurnOutcome::Success),
        Some(Value::String(outcome)) => match outcome.as_str() {
            "success" => Ok(TurnOutcome::Success),
            "failed" => Ok(TurnOutcome::Failed),
            "denied" => Ok(TurnOutcome::Denied),
            _ => Err(err(format!("unknown tool outcome: {outcome}"))),
        },
        Some(_) => Err(err("outcome must be a string")),
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

fn required_str(object: &Map<String, Value>, key: &str) -> Result<String, CodecError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| err(format!("history message needs a string {key}")))
}

/// An optional string field (missing or null reads `None`, like the
/// dataclass defaults; anything else non-string is an error).
fn opt_str(object: &Map<String, Value>, key: &str) -> Result<Option<String>, CodecError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(err(format!("{key} must be a string or null"))),
    }
}

/// An optional string field with a validation alias (the canonical
/// name wins when both are present).
fn opt_str_alias(
    object: &Map<String, Value>,
    key: &str,
    alias: &str,
) -> Result<Option<String>, CodecError> {
    if object.contains_key(key) {
        return opt_str(object, key);
    }
    opt_str(object, alias)
}

/// An optional details object (missing or null reads `None`).
fn opt_details(
    object: &Map<String, Value>,
    key: &str,
    alias: &str,
) -> Result<Option<Map<String, Value>>, CodecError> {
    let lookup = if object.contains_key(key) { key } else { alias };
    match object.get(lookup) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(map)) => Ok(Some(map.clone())),
        Some(_) => Err(err(format!("{key} must be an object or null"))),
    }
}

/// Message/part metadata (`dict | None`, default `None`): preserved
/// verbatim for the re-dump.
fn opt_metadata(object: &Map<String, Value>) -> Result<Value, CodecError> {
    match object.get("metadata") {
        None | Some(Value::Null) => Ok(Value::Null),
        Some(Value::Object(_)) => Ok(object["metadata"].clone()),
        Some(_) => Err(err("metadata must be an object or null")),
    }
}

/// `tool_call_id` (`_utils.py:431-436`): a missing key backfills with
/// a generated id, exactly the `default_factory` (which only fires on
/// absence — the field is `str`, so an explicit null fails validation).
fn call_id_or_generated(part: &Map<String, Value>) -> Result<String, CodecError> {
    match part.get("tool_call_id") {
        None => Ok(generate_tool_call_id()),
        Some(Value::String(id)) => Ok(id.clone()),
        Some(_) => Err(err("tool_call_id must be a string")),
    }
}

/// `tool_kind` (`ToolPartKind | None`): null in every capture. A
/// non-null value means a typed call/return subclass whose extra
/// fields this model cannot carry — reject loudly rather than drop
/// fields on re-dump.
fn check_null_tool_kind(part: &Map<String, Value>) -> Result<(), CodecError> {
    match part.get("tool_kind") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(kind)) => Err(err(format!("unsupported tool_kind: {kind}"))),
        Some(_) => Err(err("tool_kind must be a string or null")),
    }
}

fn parse_request_part(part: &Value) -> Result<TurnRequestPart, CodecError> {
    let object = part_object(part)?;
    match part_kind(object)? {
        "user-prompt" => match object.get("content") {
            Some(Value::String(text)) => Ok(TurnRequestPart::User {
                text: text.clone(),
                timestamp: opt_str(object, "timestamp")?,
            }),
            // Media content lists never occur in turns (the prompt is
            // the message row's plain text); reject loudly rather than
            // replaying a shape the wire mapping cannot render.
            _ => Err(err("user-prompt content must be a string")),
        },
        "tool-return" => {
            check_null_tool_kind(object)?;
            Ok(TurnRequestPart::ToolReturn {
                name: required_str(object, "tool_name")?,
                content: object
                    .get("content")
                    .cloned()
                    .ok_or_else(|| err("tool-return needs content"))?,
                call_id: call_id_or_generated(object)?,
                timestamp: opt_str(object, "timestamp")?,
                metadata: opt_metadata(object)?,
                outcome: parse_outcome(object)?,
            })
        }
        "retry-prompt" => {
            let content = match object.get("content") {
                Some(Value::String(text)) => TurnRetryContent::Text(text.clone()),
                Some(Value::Array(details)) => {
                    TurnRetryContent::Errors(parse_retry_details(details)?)
                }
                _ => return Err(err("retry-prompt content must be a string or error list")),
            };
            let tool_name = opt_str(object, "tool_name")?;
            Ok(TurnRequestPart::Retry {
                tool_name,
                content,
                call_id: call_id_or_generated(object)?,
                timestamp: opt_str(object, "timestamp")?,
            })
        }
        kind => Err(err(format!("unsupported request part_kind: {kind}"))),
    }
}

fn parse_response_part(part: &Value) -> Result<TurnResponsePart, CodecError> {
    let object = part_object(part)?;
    match part_kind(object)? {
        "text" => match object.get("content") {
            Some(Value::String(text)) => Ok(TurnResponsePart::Text {
                text: text.clone(),
                id: opt_str(object, "id")?,
                provider_name: opt_str(object, "provider_name")?,
                provider_details: opt_details(object, "provider_details", "vendor_details")?,
            }),
            _ => Err(err("text part content must be a string")),
        },
        "tool-call" => {
            check_null_tool_kind(object)?;
            let args = match object.get("args") {
                // `args` defaults to `None`: a missing key and an
                // explicit null both read `None` and re-dump as null.
                None | Some(Value::Null) => None,
                Some(Value::String(raw)) => Some(ToolArgs::Raw(raw.clone())),
                Some(Value::Object(map)) => Some(ToolArgs::Dict(map.clone())),
                // The streamed lanes store the raw arguments string;
                // the non-streaming path stores the input object;
                // anything else fails the `str | dict | None` union.
                _ => return Err(err("tool-call args must be a string, object, or null")),
            };
            Ok(TurnResponsePart::ToolCall {
                name: required_str(object, "tool_name")?,
                args,
                call_id: call_id_or_generated(object)?,
                id: opt_str(object, "id")?,
                provider_name: opt_str(object, "provider_name")?,
                provider_details: opt_details(object, "provider_details", "vendor_details")?,
            })
        }
        kind => Err(err(format!("unsupported response part_kind: {kind}"))),
    }
}

/// Parse a pydantic error-details array (`list[ErrorDetails]`, the
/// `RetryPromptPart` error form) from a raw JSON value.
///
/// The agent loop uses this to turn a tool's arg-validation failure
/// into a retry prompt without re-deriving the field rules; the stored
/// blob reader uses it through the same parser.
pub fn parse_retry_details(details: &[Value]) -> Result<Vec<RetryErrorDetail>, CodecError> {
    details.iter().map(parse_retry_detail).collect()
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
                .as_i64()
                .map(RetryLocSeg::Index)
                .ok_or_else(|| err("retry error loc index must be an int")),
            _ => Err(err("retry error loc segments must be strings or ints")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let msg = object
        .get("msg")
        .and_then(Value::as_str)
        .ok_or_else(|| err("retry error detail needs a string msg"))?
        .to_owned();
    // `url`/`ctx` never survive collection; if a foreign blob carries
    // them they are ignored, exactly the exclusion rules. `input` is
    // required (probed: `validate_python` rejects a detail without it).
    let input = object
        .get("input")
        .cloned()
        .ok_or_else(|| err("retry error detail needs input"))?;
    Ok(RetryErrorDetail {
        error_type,
        loc,
        msg,
        input,
    })
}

/// Stored per-response usage (`RequestUsage`, default zeros): the
/// seven counters plus the lane-specific `details` pre-pass. Counters
/// read numbers (ints stay ints, floats stay floats); bools read as
/// 0/1 exactly pydantic's lax ints.
fn parse_usage(object: &Map<String, Value>) -> Result<RequestUsage, CodecError> {
    let usage = match object.get("usage") {
        // `usage` is non-optional with a zeros default: a missing key
        // reads zeros, but an explicit null fails validation.
        None => return Ok(RequestUsage::zeros()),
        Some(Value::Object(map)) => map,
        Some(_) => return Err(err("usage must be an object")),
    };
    let counter = |key: &str| -> Result<UsageNumber, CodecError> {
        match usage.get(key) {
            None | Some(Value::Null) => Ok(UsageNumber::Int(0)),
            Some(Value::Number(number)) => {
                if let Some(int) = number.as_i64() {
                    Ok(UsageNumber::Int(int))
                } else if let Some(float) = number.as_f64() {
                    Ok(UsageNumber::Float(float))
                } else {
                    Err(err(format!("usage counter {key} is not a number")))
                }
            }
            Some(Value::Bool(flag)) => Ok(UsageNumber::Int(i64::from(*flag))),
            Some(_) => Err(err(format!("usage counter {key} must be a number"))),
        }
    };
    let details = match usage.get("details") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(map)) => map.clone(),
        Some(_) => return Err(err("usage details must be an object or null")),
    };
    Ok(RequestUsage {
        input_tokens: counter("input_tokens")?,
        cache_write_tokens: counter("cache_write_tokens")?,
        cache_read_tokens: counter("cache_read_tokens")?,
        output_tokens: counter("output_tokens")?,
        input_audio_tokens: counter("input_audio_tokens")?,
        cache_audio_read_tokens: counter("cache_audio_read_tokens")?,
        output_audio_tokens: counter("output_audio_tokens")?,
        details,
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
///
/// The JSON body is `error_details_ta.dump_json(..., indent=2)`
/// verbatim: 2-space pretty, raw UTF-8 (no `ensure_ascii` — the
/// serializer is serde-based, so `to_string_pretty` matches byte for
/// byte, including lowercase `\u00xx` control escapes).
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
            let body =
                serde_json::to_string_pretty(&Value::Array(errors)).unwrap_or("[]".to_owned());
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

// ---------------------------------------------------------------------------
// Dump
// ---------------------------------------------------------------------------

/// Fresh run identifiers, stamped on new messages of one run (both
/// uuid7, constant within the run, fresh across runs).
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

fn opt_string(value: &Option<String>) -> Value {
    value
        .as_ref()
        .map(|text| Value::String(text.clone()))
        .unwrap_or(Value::Null)
}

/// Dump parsed or newly built messages back into the stored shape
/// (`dump_python`, `mode='json'`): same keys, same values, same
/// `dump_python` key order.
pub fn dump_blob(messages: &[TurnMessage]) -> Value {
    Value::Array(messages.iter().map(dump_message).collect())
}

pub fn dump_message(message: &TurnMessage) -> Value {
    match message {
        TurnMessage::Request {
            parts,
            timestamp,
            instructions,
            run_id,
            conversation_id,
            metadata,
        } => Value::Object(Map::from_iter([
            (
                "parts".to_owned(),
                Value::Array(parts.iter().map(dump_request_part).collect()),
            ),
            ("timestamp".to_owned(), opt_string(timestamp)),
            ("instructions".to_owned(), opt_string(instructions)),
            ("kind".to_owned(), Value::String("request".to_owned())),
            ("run_id".to_owned(), opt_string(run_id)),
            ("conversation_id".to_owned(), opt_string(conversation_id)),
            ("metadata".to_owned(), metadata.clone()),
        ])),
        TurnMessage::Response {
            parts,
            usage,
            model_name,
            timestamp,
            provider_name,
            provider_url,
            provider_details,
            response_id,
            finish,
            run_id,
            conversation_id,
            metadata,
            state,
        } => {
            let finish_reason = finish.map(|finish| match finish {
                TurnFinish::Stop => "stop",
                TurnFinish::Length => "length",
                TurnFinish::ToolCall => "tool_call",
                TurnFinish::ContentFilter => "content_filter",
                TurnFinish::Error => "error",
            });
            let state = match state {
                TurnState::Complete => "complete",
                TurnState::Incomplete => "incomplete",
                TurnState::Interrupted => "interrupted",
            };
            Value::Object(Map::from_iter([
                (
                    "parts".to_owned(),
                    Value::Array(parts.iter().map(dump_response_part).collect()),
                ),
                ("usage".to_owned(), dump_usage(usage)),
                ("model_name".to_owned(), opt_string(model_name)),
                ("timestamp".to_owned(), Value::String(timestamp.clone())),
                ("kind".to_owned(), Value::String("response".to_owned())),
                ("provider_name".to_owned(), opt_string(provider_name)),
                ("provider_url".to_owned(), opt_string(provider_url)),
                (
                    "provider_details".to_owned(),
                    provider_details
                        .as_ref()
                        .map(|details| Value::Object(details.clone()))
                        .unwrap_or(Value::Null),
                ),
                ("provider_response_id".to_owned(), opt_string(response_id)),
                (
                    "finish_reason".to_owned(),
                    finish_reason
                        .map(|reason| Value::String(reason.to_owned()))
                        .unwrap_or(Value::Null),
                ),
                ("run_id".to_owned(), opt_string(run_id)),
                ("conversation_id".to_owned(), opt_string(conversation_id)),
                ("metadata".to_owned(), metadata.clone()),
                ("state".to_owned(), Value::String(state.to_owned())),
            ]))
        }
    }
}

fn dump_request_part(part: &TurnRequestPart) -> Value {
    match part {
        TurnRequestPart::User { text, timestamp } => Value::Object(Map::from_iter([
            ("content".to_owned(), Value::String(text.clone())),
            ("timestamp".to_owned(), opt_string(timestamp)),
            (
                "part_kind".to_owned(),
                Value::String("user-prompt".to_owned()),
            ),
        ])),
        TurnRequestPart::ToolReturn {
            name,
            content,
            call_id,
            timestamp,
            metadata,
            outcome,
        } => {
            let outcome = match outcome {
                TurnOutcome::Success => "success",
                TurnOutcome::Failed => "failed",
                TurnOutcome::Denied => "denied",
            };
            Value::Object(Map::from_iter([
                ("tool_name".to_owned(), Value::String(name.clone())),
                ("content".to_owned(), content.clone()),
                ("tool_call_id".to_owned(), Value::String(call_id.clone())),
                ("tool_kind".to_owned(), Value::Null),
                ("metadata".to_owned(), metadata.clone()),
                ("timestamp".to_owned(), opt_string(timestamp)),
                ("outcome".to_owned(), Value::String(outcome.to_owned())),
                (
                    "part_kind".to_owned(),
                    Value::String("tool-return".to_owned()),
                ),
            ]))
        }
        TurnRequestPart::Retry {
            tool_name,
            content,
            call_id,
            timestamp,
        } => Value::Object(Map::from_iter([
            ("content".to_owned(), dump_retry_content(content)),
            ("tool_name".to_owned(), opt_string(tool_name)),
            ("tool_call_id".to_owned(), Value::String(call_id.clone())),
            ("timestamp".to_owned(), opt_string(timestamp)),
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

fn dump_response_part(part: &TurnResponsePart) -> Value {
    match part {
        TurnResponsePart::Text {
            text,
            id,
            provider_name,
            provider_details,
        } => Value::Object(Map::from_iter([
            ("content".to_owned(), Value::String(text.clone())),
            ("id".to_owned(), opt_string(id)),
            ("provider_name".to_owned(), opt_string(provider_name)),
            (
                "provider_details".to_owned(),
                provider_details
                    .as_ref()
                    .map(|details| Value::Object(details.clone()))
                    .unwrap_or(Value::Null),
            ),
            ("part_kind".to_owned(), Value::String("text".to_owned())),
        ])),
        TurnResponsePart::ToolCall {
            name,
            args,
            call_id,
            id,
            provider_name,
            provider_details,
        } => {
            // Each lane's stored form: the streamed raw arguments
            // string verbatim (provider spacing intact), the
            // non-streaming input object as-is, or the stored null.
            let args = match args {
                None => Value::Null,
                Some(ToolArgs::Raw(raw)) => Value::String(raw.clone()),
                Some(ToolArgs::Dict(map)) => Value::Object(map.clone()),
            };
            Value::Object(Map::from_iter([
                ("tool_name".to_owned(), Value::String(name.clone())),
                ("args".to_owned(), args),
                ("tool_call_id".to_owned(), Value::String(call_id.clone())),
                ("tool_kind".to_owned(), Value::Null),
                ("id".to_owned(), opt_string(id)),
                ("provider_name".to_owned(), opt_string(provider_name)),
                (
                    "provider_details".to_owned(),
                    provider_details
                        .as_ref()
                        .map(|details| Value::Object(details.clone()))
                        .unwrap_or(Value::Null),
                ),
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
    use std::path::PathBuf;

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/assistant/turn-runtime")
    }

    fn load_fixture(name: &str) -> Value {
        let path = fixture_dir().join(name);
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    /// Every stored blob across the captures: `(file, index, blob)`.
    /// Single-turn fixtures carry `model_messages`; the replay fixture
    /// carries `turns[].model_messages`. Failure fixtures store null.
    fn captured_blobs() -> Vec<(String, usize, Value)> {
        let mut blobs = Vec::new();
        let mut names: Vec<String> = std::fs::read_dir(fixture_dir())
            .expect("fixture dir exists")
            .map(|entry| {
                entry
                    .expect("dir entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .filter(|name| name.ends_with(".json"))
            .collect();
        names.sort();
        assert!(!names.is_empty(), "turn-runtime fixtures are committed");
        for name in names {
            let fixture = load_fixture(&name);
            if let Some(blob) = fixture.get("model_messages") {
                if blob.is_array() {
                    blobs.push((name.clone(), 0, blob.clone()));
                }
            }
            if let Some(turns) = fixture.get("turns").and_then(Value::as_array) {
                for (index, turn) in turns.iter().enumerate() {
                    if let Some(blob) = turn.get("model_messages") {
                        if blob.is_array() {
                            blobs.push((name.clone(), index, blob.clone()));
                        }
                    }
                }
            }
        }
        blobs
    }

    #[test]
    fn round_trip_every_captured_blob() {
        let blobs = captured_blobs();
        // text x2, tool x3, retry, replay x3: every completed turn's blob.
        assert_eq!(blobs.len(), 9, "every capture contributes its blob");
        for (name, index, blob) in &blobs {
            let messages = parse_blob(blob).expect("captured blob parses");
            let dumped = dump_blob(&messages);
            // Stored blobs read back in jsonb key order while the dump
            // emits `dump_python` order (pinned separately below), so
            // equality is at the JSON-value level: same keys, same
            // values, recursively.
            assert_eq!(&dumped, blob, "{name} blob {index} must round-trip");
        }
    }

    #[test]
    fn parse_text_turn_blob() {
        let blob = &load_fixture("text_openai.json")["model_messages"];
        let messages = parse_blob(blob).expect("text blob parses");
        assert_eq!(messages.len(), 2);
        match &messages[0] {
            TurnMessage::Request {
                parts,
                instructions,
                run_id,
                ..
            } => {
                assert!(matches!(
                    parts.as_slice(),
                    [TurnRequestPart::User { text, .. }] if text == "do a thing"
                ));
                assert!(instructions
                    .as_deref()
                    .unwrap_or_default()
                    .starts_with("You are Pi Dash AI"));
                assert!(run_id.is_some());
            }
            TurnMessage::Response { .. } => panic!("first message must be a request"),
        }
        match &messages[1] {
            TurnMessage::Response {
                parts,
                usage,
                model_name,
                finish,
                provider_details,
                response_id,
                state,
                ..
            } => {
                assert!(matches!(
                    parts.as_slice(),
                    [TurnResponsePart::Text { text, .. }] if text == "Hello world"
                ));
                assert_eq!(model_name.as_deref(), Some("oracle-stub"));
                assert_eq!(*finish, Some(TurnFinish::Stop));
                assert_eq!(
                    provider_details
                        .as_ref()
                        .and_then(|details| details.get("finish_reason")),
                    Some(&json!("stop")),
                );
                assert_eq!(response_id.as_deref(), Some("stub"));
                assert_eq!(*state, TurnState::Complete);
                // The finish chunk's stream_options usage lands here.
                assert_eq!(usage.input_tokens, UsageNumber::Int(10));
                assert_eq!(usage.output_tokens, UsageNumber::Int(5));
            }
            TurnMessage::Request { .. } => panic!("second message must be a response"),
        }
    }

    #[test]
    fn parse_tool_turn_blob() {
        let blob = &load_fixture("tool_local.json")["model_messages"];
        let messages = parse_blob(blob).expect("tool blob parses");
        assert_eq!(messages.len(), 4);
        // The tool call keeps the provider's raw arguments string
        // verbatim, and the normalized finish reason.
        match &messages[1] {
            TurnMessage::Response { parts, finish, .. } => {
                assert_eq!(*finish, Some(TurnFinish::ToolCall));
                assert!(matches!(
                    parts.as_slice(),
                    [TurnResponsePart::ToolCall { name, args, call_id, .. }]
                    if name == "list_projects"
                        && matches!(args, Some(ToolArgs::Raw(raw)) if raw == "{}")
                        && call_id == "call_1"
                ));
            }
            _ => panic!("second message must be the tool-call response"),
        }
        // The tool result keeps its structured content and the success
        // outcome.
        assert!(matches!(
            messages[2],
            TurnMessage::Request { ref parts, .. }
            if matches!(parts.as_slice(),
                [TurnRequestPart::ToolReturn { ref name, ref content, ref call_id, outcome, .. }]
                if name == "list_projects"
                    && content.as_array().is_some_and(|items| !items.is_empty())
                    && call_id == "call_1"
                    && *outcome == TurnOutcome::Success)
        ));
        match &messages[3] {
            TurnMessage::Response { parts, finish, .. } => {
                assert_eq!(*finish, Some(TurnFinish::Stop));
                assert!(matches!(
                    parts.as_slice(),
                    [TurnResponsePart::Text { text, .. }] if text == "Found 1 project."
                ));
            }
            _ => panic!("fourth message must be the text response"),
        }
    }

    #[test]
    fn parse_retry_turn_blob() {
        let blob = &load_fixture("retry_validation.json")["model_messages"];
        let messages = parse_blob(blob).expect("retry blob parses");
        assert_eq!(messages.len(), 4);
        match &messages[2] {
            TurnMessage::Request { parts, .. } => {
                assert!(matches!(
                    parts.as_slice(),
                    [TurnRequestPart::Retry { tool_name, content, call_id, .. }]
                    if tool_name.as_deref() == Some("list_states")
                        && call_id == "call_bad"
                        && matches!(content, TurnRetryContent::Errors(details)
                            if matches!(details.as_slice(),
                                [RetryErrorDetail { error_type, loc, msg, input }]
                                if error_type == "missing"
                                    && loc == &[RetryLocSeg::Field("project_id".to_owned())]
                                    && msg == "Field required"
                                    && input == &json!({})))
                ));
            }
            _ => panic!("third message must be the retry request"),
        }
    }

    #[test]
    fn parse_null_finish_blob() {
        // The oracle stub sends no finish chunk: the replay's second
        // turn pins the null finish_reason shape.
        let blob = &load_fixture("replay_3turn.json")["turns"][1]["model_messages"];
        let messages = parse_blob(blob).expect("null-finish blob parses");
        match &messages[1] {
            TurnMessage::Response { finish, .. } => assert_eq!(*finish, None),
            _ => panic!("second message must be a response"),
        }
    }

    #[test]
    fn parse_anthropic_tool_call_blob() {
        // Streamed Anthropic tool_use stores the raw input JSON string,
        // like the OpenAI lane (the object form is non-streaming only).
        let blob = &load_fixture("tool_anthropic.json")["model_messages"];
        let messages = parse_blob(blob).expect("anthropic tool blob parses");
        match &messages[1] {
            TurnMessage::Response {
                parts,
                finish,
                provider_details,
                ..
            } => {
                assert_eq!(*finish, Some(TurnFinish::ToolCall));
                assert_eq!(
                    provider_details
                        .as_ref()
                        .and_then(|details| details.get("finish_reason")),
                    Some(&json!("tool_use")),
                );
                assert!(matches!(
                    parts.as_slice(),
                    [TurnResponsePart::ToolCall { name, args, call_id, .. }]
                    if name == "list_projects"
                        && matches!(args, Some(ToolArgs::Raw(raw)) if raw == "{}")
                        && call_id == "toolu_1"
                ));
            }
            _ => panic!("second message must be the tool-call response"),
        }
    }

    #[test]
    fn object_args_parse_and_dump_verbatim() {
        // The non-streaming union arm (`anthropic.py:944`): no capture
        // carries it (turns always stream), so pin the shape inline.
        let blob = json!([{
            "kind": "response",
            "timestamp": "ts",
            "parts": [{
                "part_kind": "tool-call",
                "tool_name": "list_projects",
                "args": {},
                "tool_call_id": "call_o",
            }],
        }]);
        let messages = parse_blob(&blob).expect("object args parse");
        assert!(matches!(
            messages.as_slice(),
            [TurnMessage::Response { parts, .. }]
            if matches!(parts.as_slice(),
                [TurnResponsePart::ToolCall { args, .. }]
                if matches!(args, Some(ToolArgs::Dict(map)) if map.is_empty()))
        ));
        assert_eq!(dump_blob(&messages)[0]["parts"][0]["args"], json!({}));
    }

    #[test]
    fn vendor_aliases_parse_to_canonical_dump() {
        let blob = json!([{
            "kind": "response",
            "timestamp": "ts",
            "parts": [],
            "vendor_details": {"finish_reason": "stop"},
            "vendor_id": "v1",
        }]);
        let messages = parse_blob(&blob).expect("aliases parse");
        let dumped = dump_blob(&messages);
        assert_eq!(
            dumped[0]["provider_details"],
            json!({"finish_reason": "stop"}),
        );
        assert_eq!(dumped[0]["provider_response_id"], json!("v1"));
        assert!(dumped[0].get("vendor_details").is_none());
        assert!(dumped[0].get("vendor_id").is_none());
    }

    #[test]
    fn outcome_state_and_error_finish_round_trip() {
        // Literal arms no capture reaches, pinned inline.
        let blob = json!([
            {
                "kind": "request",
                "parts": [
                    {
                        "part_kind": "tool-return",
                        "tool_name": "t",
                        "content": {},
                        "tool_call_id": "c",
                        "outcome": "denied",
                    },
                    {
                        "part_kind": "tool-return",
                        "tool_name": "t",
                        "content": {},
                        "tool_call_id": "c",
                        "outcome": "failed",
                    },
                ],
            },
            {
                "kind": "response",
                "timestamp": "ts",
                "parts": [],
                "finish_reason": "error",
                "state": "interrupted",
            },
            {
                "kind": "response",
                "timestamp": "ts",
                "parts": [],
                "state": "incomplete",
            },
        ]);
        let messages = parse_blob(&blob).expect("literal arms parse");
        let dumped = dump_blob(&messages);
        // Sparse inline blob: assert the literal arms, then the dump
        // fixpoint (dumped output re-parses to the identical dump).
        assert_eq!(dumped[0]["parts"][0]["outcome"], json!("denied"));
        assert_eq!(dumped[0]["parts"][1]["outcome"], json!("failed"));
        assert_eq!(dumped[1]["finish_reason"], json!("error"));
        assert_eq!(dumped[1]["state"], json!("interrupted"));
        assert_eq!(dumped[2]["state"], json!("incomplete"));
        let back = parse_blob(&dumped).expect("dump re-parses");
        assert_eq!(dump_blob(&back), dumped);
        assert!(matches!(
            messages[1],
            TurnMessage::Response {
                finish: Some(TurnFinish::Error),
                state: TurnState::Interrupted,
                ..
            }
        ));
    }

    #[test]
    fn usage_counters_parse_numbers_and_bools() {
        let blob = json!([{
            "kind": "response",
            "timestamp": "ts",
            "parts": [],
            "usage": {
                "input_tokens": 3,
                "output_tokens": 1.5,
                "cache_read_tokens": true,
                "details": {"input_tokens": 3},
            },
        }]);
        let messages = parse_blob(&blob).expect("usage parses");
        match &messages[0] {
            TurnMessage::Response { usage, .. } => {
                assert_eq!(usage.input_tokens, UsageNumber::Int(3));
                assert_eq!(usage.output_tokens, UsageNumber::Float(1.5));
                assert_eq!(usage.cache_read_tokens, UsageNumber::Int(1));
                assert_eq!(usage.cache_write_tokens, UsageNumber::Int(0));
                assert_eq!(usage.details.get("input_tokens"), Some(&json!(3)),);
            }
            _ => panic!("must be a response"),
        }
        let dumped = dump_blob(&messages);
        assert_eq!(
            dumped[0]["usage"],
            json!({
                "input_tokens": 3,
                "cache_write_tokens": 0,
                "cache_read_tokens": 1,
                "output_tokens": 1.5,
                "input_audio_tokens": 0,
                "cache_audio_read_tokens": 0,
                "output_audio_tokens": 0,
                "details": {"input_tokens": 3},
            }),
        );
        let back = parse_blob(&dumped).expect("dump re-parses");
        assert_eq!(dump_blob(&back), dumped);
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
            "timestamp": "ts",
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
        // Non-null tool_kind: a typed part whose extra fields the
        // model cannot carry; accepting it would drop fields on dump.
        assert!(parse_blob(&json!([{
            "kind": "response",
            "timestamp": "ts",
            "parts": [{
                "part_kind": "tool-call",
                "tool_name": "t",
                "args": "{}",
                "tool_kind": "tool-search",
            }],
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
            "timestamp": "ts",
            "parts": [{"part_kind": "text", "content": "x"}],
            "finish_reason": "warp",
        }]))
        .is_err());
        // Retry details require `input` (probed).
        assert!(parse_blob(&json!([{
            "kind": "request",
            "parts": [{
                "part_kind": "retry-prompt",
                "content": [{"type": "missing", "loc": ["x"], "msg": "m"}],
            }],
        }]))
        .is_err());
        // Response timestamps have no faithful default.
        assert!(parse_blob(&json!([{
            "kind": "response",
            "parts": [],
        }]))
        .is_err());
        // Missing tool_call_id backfills (the default_factory).
        let messages = parse_blob(&json!([{
            "kind": "response",
            "timestamp": "ts",
            "parts": [{"part_kind": "tool-call", "tool_name": "t", "args": "{}"}],
        }]))
        .expect("missing id backfills");
        assert!(matches!(
            messages.as_slice(),
            [TurnMessage::Response { parts, .. }]
            if matches!(parts.as_slice(), [TurnResponsePart::ToolCall { call_id, .. }]
                if call_id.starts_with("pyd_ai_"))
        ));
        // ...but an explicit null fails validation (the field is `str`).
        assert!(parse_blob(&json!([{
            "kind": "response",
            "timestamp": "ts",
            "parts": [{
                "part_kind": "tool-call",
                "tool_name": "t",
                "args": "{}",
                "tool_call_id": Value::Null,
            }],
        }]))
        .is_err());
        // An explicit null usage fails validation too (missing reads zeros).
        assert!(parse_blob(&json!([{
            "kind": "response",
            "timestamp": "ts",
            "parts": [],
            "usage": Value::Null,
        }]))
        .is_err());
    }

    #[test]
    fn join_valid_blobs_skips_undecodable_turn() {
        // `history.py:45-52`: each blob validates on its own; a blob that
        // fails is skipped with a warning, so one undecodable stored turn
        // never fails the turns replaying after it (PIDASHCONV-1569).
        let valid = json!([
            {
                "kind": "request",
                "parts": [{"part_kind": "user-prompt", "content": "hi"}],
            },
            {
                "kind": "response",
                "timestamp": "ts",
                "parts": [{"part_kind": "text", "content": "hello"}],
            },
        ]);
        let thinking = json!([{
            "kind": "response",
            "timestamp": "ts",
            "parts": [{"part_kind": "thinking", "content": "hmm"}],
        }]);
        let blobs = [
            valid.clone(),
            thinking,
            Value::Null,
            json!([]),
            json!({"kind": "request"}),
            valid.clone(),
        ];
        let joined = join_valid_blobs(&blobs);
        let expected: Vec<Value> = valid
            .as_array()
            .expect("valid blob is an array")
            .iter()
            .cloned()
            .chain(
                valid
                    .as_array()
                    .expect("valid blob is an array")
                    .iter()
                    .cloned(),
            )
            .collect();
        assert_eq!(joined, expected);
        // The joined list holds only blobs that parsed, so the later
        // single-pass parse (`open_stream`) cannot fail on a skipped turn.
        assert!(parse_blob(&Value::Array(joined)).is_ok());
    }

    #[test]
    fn null_args_parse_and_dump_as_null() {
        // A streamed `tool_use` block with zero input deltas stores
        // null (`anthropic.py:2222`); `validate_python` accepts it and
        // `dump_python` reproduces it.
        for blob in [
            json!([{
                "kind": "response",
                "timestamp": "ts",
                "parts": [{
                    "part_kind": "tool-call",
                    "tool_name": "t",
                    "args": Value::Null,
                    "tool_call_id": "c",
                }],
            }]),
            json!([{
                "kind": "response",
                "timestamp": "ts",
                "parts": [{
                    "part_kind": "tool-call",
                    "tool_name": "t",
                    "tool_call_id": "c",
                }],
            }]),
        ] {
            let messages = parse_blob(&blob).expect("null args parse");
            assert!(matches!(
                messages.as_slice(),
                [TurnMessage::Response { parts, .. }]
                if matches!(parts.as_slice(),
                    [TurnResponsePart::ToolCall { args: None, .. }])
            ));
            let dumped = dump_blob(&messages);
            assert_eq!(dumped[0]["parts"][0]["args"], Value::Null);
            let back = parse_blob(&dumped).expect("dump re-parses");
            assert_eq!(dump_blob(&back), dumped);
        }
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
    fn retry_errors_keep_raw_utf8_like_dump_json() {
        // Byte pin against `RetryPromptPart(...).model_response()`
        // (pydantic-ai 1.107 venv probe): `dump_json` emits raw UTF-8,
        // never `\uXXXX` escapes.
        let content = TurnRetryContent::Errors(vec![RetryErrorDetail {
            error_type: "missing".to_owned(),
            loc: vec![RetryLocSeg::Field("x".to_owned())],
            msg: "café 中".to_owned(),
            input: json!({}),
        }]);
        assert_eq!(
            retry_message(&content, Some("t")),
            "1 validation error:\n```json\n[\n  {\n    \"type\": \"missing\",\n    \"loc\": [\n      \"x\"\n    ],\n    \"msg\": \"café 中\",\n    \"input\": {}\n  }\n]\n```\n\nFix the errors and try again."
        );
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
            timestamp: Some("ts".to_owned()),
        };
        let ids = RunIds {
            run_id: "r".to_owned(),
            conversation_id: "c".to_owned(),
        };
        let dumped = dump_message(&TurnMessage::new_request(
            vec![part.clone()],
            Some("instr".to_owned()),
            &ids,
            "ts",
        ));
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
                "timestamp": "ts",
            }],
        }]))
        .expect("dumped retry parses");
        assert!(matches!(
            back.as_slice(),
            [TurnMessage::Request { parts, .. }] if parts == std::slice::from_ref(&part)
        ));
    }

    #[test]
    fn new_messages_dump_in_stored_shape() {
        // The loop's construction path: fresh stamps in, stored shape
        // out, parsing back to the same model.
        let ids = RunIds::fresh();
        let request = TurnMessage::new_request(
            vec![TurnRequestPart::User {
                text: "hi".to_owned(),
                timestamp: Some("ts".to_owned()),
            }],
            Some("instr".to_owned()),
            &ids,
            "ts",
        );
        let response = TurnMessage::new_response(
            vec![TurnResponsePart::Text {
                text: "hi".to_owned(),
                id: None,
                provider_name: None,
                provider_details: None,
            }],
            RequestUsage::zeros(),
            Some("m".to_owned()),
            "ts",
            Some("openai".to_owned()),
            Some("http://stub/v1/".to_owned()),
            None,
            Some("r1".to_owned()),
            Some(TurnFinish::Stop),
            &ids,
        );
        let dumped = dump_blob(&[request, response]);
        let back = parse_blob(&dumped).expect("dumped messages parse");
        assert_eq!(dump_blob(&back), dumped);
        assert_eq!(dumped[0]["run_id"], dumped[1]["run_id"]);
        assert_eq!(dumped[1]["state"], json!("complete"));
    }

    #[test]
    fn dump_key_order_follows_capture() {
        let ids = RunIds {
            run_id: "r".to_owned(),
            conversation_id: "c".to_owned(),
        };
        let request = dump_message(&TurnMessage::new_request(
            vec![TurnRequestPart::User {
                text: "hi".to_owned(),
                timestamp: Some("ts".to_owned()),
            }],
            Some("instr".to_owned()),
            &ids,
            "ts",
        ));
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
        let response = dump_message(&TurnMessage::new_response(
            vec![TurnResponsePart::Text {
                text: "hi".to_owned(),
                id: None,
                provider_name: None,
                provider_details: None,
            }],
            RequestUsage::zeros(),
            Some("m".to_owned()),
            "ts",
            Some("openai".to_owned()),
            Some("http://stub/v1/".to_owned()),
            None,
            None,
            Some(TurnFinish::Stop),
            &ids,
        ));
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
