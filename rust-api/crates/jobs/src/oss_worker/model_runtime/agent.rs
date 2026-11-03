//! The agent loop: `Agent.run` as constructed by
//! `cloud_agent/runtime.py:18-76` (`execute`), i.e. the `_agent_graph.py`
//! `HandleResponse` flow for `output_type=CloudAgentOutput`,
//! `retries=2`, `tool_timeout=20`, `end_strategy='early'`, with no
//! capabilities, approvals, deferrals, or native tools.
//!
//! The ported rules, each with its probe or graph line:
//!
//! * One model request per turn: `check_before_request`, POST, usage
//!   accumulation (`requests += 1` per parsed response only),
//!   `check_tokens` (`usage.py:92-157`).
//! * Empty responses (`loopprobe.json`, `_agent_graph.py:1107-1171`):
//!   `length` raises the token-limit error naming the last
//!   `max_tokens`; `content_filter` raises `ContentFilterError` with
//!   the pretty-printed message body; anything else resubmits an
//!   empty request on output budget.
//! * Tool calls (`process_tool_calls`, `_agent_graph.py:1523-1860`):
//!   output calls validate first — failures append a retry and
//!   consume output budget, the first success ends the turn; with
//!   `early` + a final result the remaining function calls stub out
//!   (`Tool not executed ...`, uncounted) while unknown calls still
//!   process; otherwise the usage gate runs over the projected
//!   count, validation runs sequentially in call order, and the
//!   valid calls execute concurrently with results appended in call
//!   order (`_call_tools`, first-completed error wins, siblings
//!   dropped).
//! * Tool retries (`tool_manager.py:122-140,177-181,407-414`):
//!   per-name failure counts are consecutive — each step rebuilds
//!   the map from the previous step's failed set only, so a success
//!   (or a step without that tool failing) resets the count; at the
//!   max the turn raises `Tool 'name' exceeded max retries count of
//!   N`. Function, MCP, and unknown tools all get 2: MCP resolves
//!   `max_retries=None` from `ctx.max_retries` (`mcp.py:2450`, the
//!   run-step default), and unknown names use `default_max_retries`.
//! * Tool errors: `ModelRetry` retries with its message; anything
//!   else propagates raw (`loopprobe.json` `tool_exc`).
//! * Sync function tools have no effective timeout: the toolset's
//!   `anyio.fail_after` cannot interrupt the `to_thread` wait
//!   (anyio 4.15 `cancellable=False`), proven by `timeoutprobe.json`
//!   (a 5s tool with `tool_timeout=1` succeeds at 5.2s). The port
//!   runs them to completion. MCP calls keep their real timeout
//!   (`process_tool_call`'s `asyncio.timeout`), which surfaces as
//!   `TimeoutError` and classifies `run_timeout`.
//! * Output extraction: text is discarded whenever tool calls are
//!   present (`finalprobe.json` `text_plus_tool`); otherwise the
//!   concatenated texts (current turn, else recovered from history)
//!   validate as output after fence-stripping
//!   (`strip_markdown_fences`, `_utils.py:1091-1100`).
//! * Output retries share one counter (`output_retries_used`, 3
//!   attempts at `retries=2`), spent by text failures, output-tool
//!   failures, and empty resubmits alike — but only the text/empty
//!   paths enforce the max; the output-tool path only increments
//!   (`_agent_graph.py:1624`) and exhausts via its consecutive
//!   per-name count, relabeled to the global message.
//! * Either exhaustion path first checks for a length-truncated
//!   tool call and raises `IncompleteToolCall` instead
//!   (`check_incomplete_tool_call`).

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use super::provider::{
    generate_tool_call_id, Finish, HistoryMessage, Lane, ParsedResponse, RequestPart, ResponsePart,
    RetryContent, ToolArgs, ToolCall, WireTool,
};
use super::schemas::ToolSchema;
use super::usage::{LimitBreach, ProviderTable, RequestUsage, RunUsage};
use super::validation::{ValidatedArgs, ValidationDetail};

/// `final_result`: the output tool name (`_output.py`).
pub const OUTPUT_TOOL_NAME: &str = "final_result";

/// The textless no-recovery retry content: `Please {" or
/// ".join(alternatives)}.` for this configuration (output toolset +
/// `return text`).
const TEXTLESS_RETRY_TEXT: &str = "Please return text or include your response in a tool call.";

/// How one tool call resolves (`get_tool_def` /
/// `_classify_tool_calls`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    /// A `build_tools` function tool: lax schema validation with
    /// retries, then the executor.
    Function,
    /// A GitHub MCP tool: dict-shape check with retries, then the
    /// in-process FastMCP leg (jsonschema validation, masked
    /// errors, real timeout).
    Mcp,
    /// The output tool: output validation on output budget.
    Output,
    /// Anything else: the unknown-tool retry.
    Unknown,
}

/// What a tool execution yields. Function tools only ever return
/// `Ok` or `Fatal` (no `ModelRetry`, no effective timeout); the MCP
/// leg adds masked `Fatal` text and `Timeout`.
pub enum ToolOutcome {
    Ok(Value),
    // Only test fakes return this — production twins mask it to
    // `Fatal` first — but the executor contract keeps the variant.
    #[allow(dead_code)]
    ModelRetry(String),
    Fatal(String),
    Timeout,
}

/// The tool executor seam: the loop validates and retries; the
/// executor runs. Production wiring lives in `tools_exec`; tests
/// inject fakes.
pub trait ToolExecutor: Send + Sync {
    /// Run one validated function tool (`Tool.call_tool`).
    fn call_function(
        &self,
        name: &str,
        args: ValidatedArgs,
        call_id: &str,
    ) -> impl std::future::Future<Output = ToolOutcome> + Send;
    /// Run one MCP tool's sync twin (`mcp_add` body): the loop has
    /// already jsonschema-validated `args`. `call_id` is the MCP
    /// call id, but the twin resolves the ledger id from `None`
    /// (fresh v4): the contextvar `process_tool_call` sets never
    /// reaches the twin across the in-process hop (`verify2.py`
    /// `b_ctx`; function tools likewise never see it set).
    fn call_mcp(
        &self,
        name: &str,
        args: Map<String, Value>,
        call_id: &str,
    ) -> impl std::future::Future<Output = ToolOutcome> + Send;
}

/// The loop's static inputs: one `ModelInvocation` resolved against
/// the golden schemas.
pub struct LoopConfig<'a> {
    pub lane: &'a Lane,
    pub client: &'a reqwest::Client,
    pub extract: &'a ProviderTable,
    pub instructions: &'a str,
    pub prompt: &'a str,
    /// Wire tools in registration order (fn sorted + MCP source
    /// order + `final_result` last).
    pub wire_tools: &'a [WireTool],
    /// Function-tool schemas by name (lax validation).
    pub function_schemas: &'a HashMap<String, ToolSchema>,
    /// MCP-tool schemas by name (jsonschema validation).
    pub mcp_schemas: &'a HashMap<String, ToolSchema>,
    /// Granted MCP names (unknown names beyond these still resolve
    /// as unknown, never as MCP).
    pub mcp_names: &'a HashSet<String>,
    pub max_tool_retries: u64,
    pub max_output_retries: u64,
    /// MCP tools retry like function tools: our `MCPToolset` passes
    /// `max_retries=None`, which resolves per-tool from
    /// `ctx.max_retries` (`mcp.py:2450`), i.e. the run-step default
    /// — not the standalone-construction default of 1 (`mcp.py:814`).
    pub mcp_max_retries: u64,
    pub limits: &'a crate::dispatch_execute::UsageLimits,
    pub max_tokens: i64,
    pub timeout_secs: u64,
    /// MCP call timeout, seconds (`read_timeout` /
    /// `CLOUD_AGENT_TOOL_TIMEOUT_SECONDS`).
    pub mcp_timeout_secs: u64,
}

/// What the loop returns: the structured output plus accumulated
/// usage (`result.output`, `result.usage()`).
#[derive(Debug)]
pub struct LoopOutput {
    pub output: pidash_types::dispatch::CloudAgentOutput,
    pub usage: RunUsage,
}

/// What the loop raises, before `invoke` maps it onto
/// `InvokeError`.
#[derive(Debug)]
pub enum LoopError {
    /// A usage-limit message, verbatim (`UsageLimitExceeded`).
    UsageLimit(String),
    /// An MCP tool timed out (`TimeoutError` → `run_timeout`).
    Timeout,
    /// Anything else: provider, exhaustion, filter, tool-fatal.
    Failed(String),
}

impl LoopError {
    fn failed(message: impl Into<String>) -> Self {
        LoopError::Failed(message.into())
    }
}

impl From<LimitBreach> for LoopError {
    fn from(breach: LimitBreach) -> Self {
        LoopError::UsageLimit(breach.0)
    }
}

/// Run the loop to a final output (`Agent.run`).
pub async fn run_loop<E: ToolExecutor>(
    config: &LoopConfig<'_>,
    executor: &E,
) -> Result<LoopOutput, LoopError> {
    let mut state = LoopState::new();
    state.history.push(HistoryMessage::Request {
        parts: vec![RequestPart::UserPrompt(config.prompt.to_owned())],
    });
    loop {
        super::usage::check_before_request(config.limits, &state.usage)?;
        let turn = super::provider::Turn {
            lane: config.lane,
            instructions: config.instructions,
            history: &state.history,
            tools: config.wire_tools,
            max_tokens: config.max_tokens,
            timeout_secs: config.timeout_secs,
            extract: config.extract,
        };
        let response = match super::provider::complete(config.client, &turn).await {
            Ok(response) => response,
            Err(error) => return Err(LoopError::failed(error.message(config.lane))),
        };
        state.usage.incr(&response.usage);
        state.usage.requests += 1;
        super::usage::check_tokens(config.limits, &state.usage)?;
        state.last_max_tokens = Some(config.max_tokens);

        if response.parts.is_empty() {
            // `HandleResponse` empty arms (`_agent_graph.py:1107-1171`).
            if response.finish == Some(Finish::Length) {
                return Err(LoopError::failed(length_message(state.last_max_tokens)));
            }
            if response.finish == Some(Finish::ContentFilter) {
                return Err(LoopError::failed(content_filter_message(
                    config.lane,
                    &state,
                    &response,
                )));
            }
            state.consume_output_retry(config.max_output_retries)?;
            state.reset_tool_retries();
            state.history.push(response_message(&response));
            state
                .history
                .push(HistoryMessage::Request { parts: Vec::new() });
            continue;
        }

        // The length arm also fires for thinking-only responses
        // (`_agent_graph.py:1107-1116`); anything else falls through
        // to the normal handling below, which excludes thinking from
        // the text and ends at the retry prompt when nothing
        // recovers (`:1170-1197`).
        let thinking_only = response
            .parts
            .iter()
            .all(|part| matches!(part, ResponsePart::Thinking { .. }));
        if thinking_only && response.finish == Some(Finish::Length) {
            return Err(LoopError::failed(length_message(state.last_max_tokens)));
        }

        let mut texts: Vec<&str> = Vec::new();
        let mut calls: Vec<&ToolCall> = Vec::new();
        for part in &response.parts {
            match part {
                ResponsePart::Text(text) => texts.push(text.as_str()),
                // `ThinkingPart` is excluded from the text
                // (`_agent_graph.py:1197`).
                ResponsePart::Thinking { .. } => {}
                ResponsePart::ToolCall(call) => calls.push(call),
            }
        }
        if !calls.is_empty() {
            // Text alongside tool calls is discarded (kept in
            // history for `_recover_text_from_message_history`).
            state.history.push(response_message(&response));
            let outcome = process_tool_calls(config, executor, &mut state, &calls).await?;
            state.history.push(HistoryMessage::Request {
                parts: outcome.parts,
            });
            if let Some(output) = outcome.final_result {
                return Ok(LoopOutput {
                    output,
                    usage: state.usage,
                });
            }
            continue;
        }

        // No tool calls: the concatenated text, recovered from
        // history when the turn itself is textless
        // (`_agent_graph.py:1192-1258`). The response lands in
        // history before handling (`_append_response`), so the
        // recovery scan sees it — textless, hence skipped.
        let text = texts.concat();
        state.history.push(response_message(&response));
        let recovered = if text.is_empty() {
            recover_text(&state.history)
        } else {
            Some(text)
        };
        let Some(text) = recovered else {
            // Files-only responses cannot occur here (no file
            // parts exist); the alternatives arm still reads
            // faithfully.
            state.consume_output_retry(config.max_output_retries)?;
            state.reset_tool_retries();
            state.history.push(HistoryMessage::Request {
                parts: vec![RequestPart::Retry {
                    tool_name: None,
                    content: RetryContent::Text(TEXTLESS_RETRY_TEXT.to_owned()),
                    call_id: generate_tool_call_id(),
                }],
            });
            continue;
        };
        match validate_output_text(&text) {
            Ok(output) => {
                return Ok(LoopOutput {
                    output,
                    usage: state.usage,
                });
            }
            Err(details) => {
                let retry = RequestPart::Retry {
                    tool_name: None,
                    content: RetryContent::Errors(details),
                    call_id: generate_tool_call_id(),
                };
                state.consume_output_retry(config.max_output_retries)?;
                state.reset_tool_retries();
                state
                    .history
                    .push(HistoryMessage::Request { parts: vec![retry] });
            }
        }
    }
}

/// Mutable loop state (`GraphAgentState` subset).
struct LoopState {
    history: Vec<HistoryMessage>,
    usage: RunUsage,
    /// Per-tool failure counts (`ToolManager.ctx.retries`).
    retries: HashMap<String, u64>,
    output_retries_used: u64,
    last_max_tokens: Option<i64>,
    run_id: String,
    conversation_id: String,
}

impl LoopState {
    fn new() -> Self {
        LoopState {
            history: Vec::new(),
            usage: RunUsage::default(),
            retries: HashMap::new(),
            output_retries_used: 0,
            last_max_tokens: None,
            // `uuid7()` (`_agent_graph.py` run/conversation ids).
            run_id: uuid::Uuid::now_v7().to_string(),
            conversation_id: uuid::Uuid::now_v7().to_string(),
        }
    }

    /// `consume_output_retry`: spend one unit of output budget,
    /// raising the exhaustion error past the max (after the
    /// truncated-call check).
    fn consume_output_retry(&mut self, max_output_retries: u64) -> Result<(), LoopError> {
        self.output_retries_used += 1;
        if self.output_retries_used > max_output_retries {
            if let Some(message) = self.check_incomplete_tool_call() {
                return Err(LoopError::failed(message));
            }
            return Err(LoopError::failed(format!(
                "Exceeded maximum output retries ({max_output_retries})"
            )));
        }
        Ok(())
    }

    /// `check_incomplete_tool_call`: the truncated-call message when
    /// the last response ended `length` on an invalid tool call.
    fn check_incomplete_tool_call(&self) -> Option<String> {
        let HistoryMessage::Response { parts, finish, .. } = self.history.last()? else {
            return None;
        };
        if *finish != Some(Finish::Length) {
            return None;
        }
        let ResponsePart::ToolCall(call) = parts.last()? else {
            return None;
        };
        if tool_args_valid(call) {
            return None;
        }
        let max_tokens = self
            .last_max_tokens
            .map(|max| max.to_string())
            .unwrap_or_else(|| "provider default".to_owned());
        Some(format!(
            "Model token limit ({max_tokens}) exceeded while generating a tool call, resulting in incomplete arguments. Increase the `max_tokens` model setting, or simplify the prompt to result in a shorter response that will fit within the limit."
        ))
    }

    /// A step with no tool failures still rebuilds the retry map
    /// (`for_run_step` with an empty failed set): every count
    /// resets.
    fn reset_tool_retries(&mut self) {
        self.retries.clear();
    }

    /// `_check_max_retries`: raise the per-tool exhaustion error at
    /// the max.
    fn check_max_retries(&self, name: &str, max_retries: u64) -> Result<(), LoopError> {
        if self.retries.get(name).copied().unwrap_or(0) == max_retries {
            if let Some(message) = self.check_incomplete_tool_call() {
                return Err(LoopError::failed(message));
            }
            return Err(LoopError::failed(format!(
                "Tool '{name}' exceeded max retries count of {max_retries}"
            )));
        }
        Ok(())
    }
}

/// Whether a tool call's arguments parse as an object
/// (`args_as_dict(raise_if_invalid=True)` surviving).
fn tool_args_valid(call: &ToolCall) -> bool {
    match &call.args {
        ToolArgs::Dict(_) => true,
        ToolArgs::Raw(raw) => {
            matches!(serde_json::from_str::<Value>(raw), Ok(Value::Object(_)))
        }
    }
}

/// The length-exhaustion message (`_agent_graph.py:1123-1130`).
fn length_message(last_max_tokens: Option<i64>) -> String {
    let max_tokens = last_max_tokens
        .map(|max| max.to_string())
        .unwrap_or_else(|| "provider default".to_owned());
    format!(
        "Model token limit ({max_tokens}) exceeded before any response was generated. Increase the `max_tokens` model setting, or simplify the prompt to result in a shorter response that will fit within the limit."
    )
}

/// The content-filter message (`_agent_graph.py:1117-1129` +
/// `UnexpectedModelBehavior.__str__`): the finish reason, block
/// reason, or `repr`'d refusal plus the pretty-printed response
/// body (`json.dumps(..., indent=2)`, ASCII-escaped).
fn content_filter_message(lane: &Lane, state: &LoopState, response: &ParsedResponse) -> String {
    let details_empty = Map::new();
    let details = response.provider_details.as_ref().unwrap_or(&details_empty);
    // Each arm reads a truthy value (`:=` in Python): empty strings
    // fall through.
    let present = |key: &str| {
        details
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    };
    let message = if let Some(reason) = present("finish_reason") {
        format!("Content filter triggered. Finish reason: '{reason}'")
    } else if let Some(blocked) = present("block_reason") {
        format!("Content filter triggered. Block reason: '{blocked}'")
    } else if let Some(refusal) = present("refusal") {
        format!(
            "Content filter triggered. Refusal: {}",
            super::provider::py_repr(&Value::String(refusal.to_owned()))
        )
    } else {
        "Content filter triggered.".to_owned()
    };
    let body = Value::Array(vec![response_json(lane, state, response)]);
    let pretty = serde_json::to_string_pretty(&body).unwrap_or_default();
    let pretty = super::validation::ensure_ascii(&pretty);
    format!("{message}, body:\n{pretty}")
}

/// The `ModelResponse` JSON inside the filter body
/// (`ModelMessagesTypeAdapter.dump_json`): dataclass key order, the
/// actual response usage, nulls included.
fn response_json(lane: &Lane, state: &LoopState, response: &ParsedResponse) -> Value {
    let parts: Vec<Value> = response
        .parts
        .iter()
        .map(|part| match part {
            ResponsePart::Text(text) => Value::Object(Map::from_iter([
                ("content".to_owned(), Value::String(text.clone())),
                ("id".to_owned(), Value::Null),
                ("provider_name".to_owned(), Value::Null),
                ("provider_details".to_owned(), Value::Null),
                ("part_kind".to_owned(), Value::String("text".to_owned())),
            ])),
            // `ThinkingPart` dataclass order (`messages.py:1629+`):
            // content, id, signature, provider_name, provider_details,
            // part kind. Unreachable in practice: the filter arm only
            // runs on empty responses.
            ResponsePart::Thinking { id, content } => Value::Object(Map::from_iter([
                ("content".to_owned(), Value::String(content.clone())),
                ("id".to_owned(), Value::String(id.clone())),
                ("signature".to_owned(), Value::Null),
                (
                    "provider_name".to_owned(),
                    Value::String(lane.provider_name().to_owned()),
                ),
                ("provider_details".to_owned(), Value::Null),
                ("part_kind".to_owned(), Value::String("thinking".to_owned())),
            ])),
            ResponsePart::ToolCall(call) => Value::Object(Map::from_iter([
                ("tool_name".to_owned(), Value::String(call.name.clone())),
                (
                    "args".to_owned(),
                    match &call.args {
                        ToolArgs::Dict(map) => Value::Object(map.clone()),
                        ToolArgs::Raw(raw) => Value::String(raw.clone()),
                    },
                ),
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
            ])),
        })
        .collect();
    Value::Object(Map::from_iter([
        ("parts".to_owned(), Value::Array(parts)),
        ("usage".to_owned(), usage_json(&response.usage)),
        (
            "model_name".to_owned(),
            Value::String(response.model_name.clone()),
        ),
        (
            "timestamp".to_owned(),
            Value::String(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)),
        ),
        ("kind".to_owned(), Value::String("response".to_owned())),
        (
            "provider_name".to_owned(),
            Value::String(lane.provider_name().to_owned()),
        ),
        (
            "provider_url".to_owned(),
            Value::String(lane.provider_url()),
        ),
        (
            "provider_details".to_owned(),
            response
                .provider_details
                .clone()
                .map(Value::Object)
                .unwrap_or(Value::Null),
        ),
        (
            "provider_response_id".to_owned(),
            response
                .response_id
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        ),
        (
            "finish_reason".to_owned(),
            response
                .finish
                .map(|finish| {
                    Value::String(
                        match finish {
                            Finish::Stop => "stop",
                            Finish::Length => "length",
                            Finish::ToolCall => "tool_call",
                            Finish::ContentFilter => "content_filter",
                        }
                        .to_owned(),
                    )
                })
                .unwrap_or(Value::Null),
        ),
        ("run_id".to_owned(), Value::String(state.run_id.clone())),
        (
            "conversation_id".to_owned(),
            Value::String(state.conversation_id.clone()),
        ),
        ("metadata".to_owned(), Value::Null),
        ("state".to_owned(), Value::String("complete".to_owned())),
    ]))
}

/// The `RequestUsage` JSON shape in dataclass field order
/// (`model_dump`).
fn usage_json(usage: &RequestUsage) -> Value {
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

/// Recover discarded text from history
/// (`_recover_text_from_message_history`): scan every response
/// newest-first; the first response whose concatenated text is
/// non-empty wins. (No `NativeToolCallPart` reset: native tools never
/// occur in this configuration.)
fn recover_text(history: &[HistoryMessage]) -> Option<String> {
    for message in history.iter().rev() {
        if let HistoryMessage::Response { parts, .. } = message {
            let text: String = parts
                .iter()
                .filter_map(|part| match part {
                    ResponsePart::Text(text) => Some(text.as_str()),
                    ResponsePart::Thinking { .. } | ResponsePart::ToolCall(_) => None,
                })
                .collect();
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

/// Wrap a parsed response as a history message.
fn response_message(response: &ParsedResponse) -> HistoryMessage {
    HistoryMessage::Response {
        parts: response.parts.clone(),
        usage: response.usage.clone(),
        model_name: response.model_name.clone(),
        finish: response.finish,
        provider_details: response.provider_details.clone(),
        response_id: response.response_id.clone(),
    }
}

/// Validate output text: fence-strip, then `validate_json(data or
/// '{}')` (`_output.py:940`, `_validate_text_output`).
fn validate_output_text(
    text: &str,
) -> Result<pidash_types::dispatch::CloudAgentOutput, Vec<ValidationDetail>> {
    let stripped = strip_markdown_fences(text);
    let payload = if stripped.is_empty() { "{}" } else { stripped };
    match serde_json::from_str::<Value>(payload) {
        Ok(value) => super::validation::validate_output(&value),
        Err(error) => Err(vec![ValidationDetail {
            error_type: "json_invalid",
            loc: Vec::new(),
            msg: format!("Invalid JSON: {error}"),
            input: Value::String(payload.to_owned()),
        }]),
    }
}

/// `strip_markdown_fences` (`_utils.py:1094-1100`): texts starting
/// with `{` read as-is, else the first fenced `{...}` group wins.
fn strip_markdown_fences(text: &str) -> &str {
    if text.starts_with('{') {
        return text;
    }
    static FENCES: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pattern = FENCES.get_or_init(|| {
        // `(?s)`: `re.DOTALL` — `{...}` spans newlines. `\Z`
        // (absolute end) reads `\z` in Rust syntax.
        regex::Regex::new(r"(?s)```(?:\w+)?\n(\{.*?\})\s*(?:\n?```|\z)").unwrap()
    });
    pattern
        .captures(text)
        .and_then(|captures| captures.get(1))
        .map(|found| found.as_str())
        .unwrap_or(text)
}

// ---------------------------------------------------------------------------
// Tool-call processing
// ---------------------------------------------------------------------------

/// What one turn's tool calls produce: the follow-up request parts
/// plus the final output when an output call landed.
struct TurnOutcome {
    parts: Vec<RequestPart>,
    final_result: Option<pidash_types::dispatch::CloudAgentOutput>,
}

/// `process_tool_calls` for this configuration
/// (`_agent_graph.py:1523-1860`): output calls first, then the
/// gated concurrent function batch; unknown calls always process.
async fn process_tool_calls<E: ToolExecutor>(
    config: &LoopConfig<'_>,
    executor: &E,
    state: &mut LoopState,
    calls: &[&ToolCall],
) -> Result<TurnOutcome, LoopError> {
    let mut output_parts: Vec<RequestPart> = Vec::new();
    let mut final_result: Option<pidash_types::dispatch::CloudAgentOutput> = None;
    let mut function_calls: Vec<&ToolCall> = Vec::new();
    let mut unknown_calls: Vec<&ToolCall> = Vec::new();
    // Failed names rebuild every step (`for_run_step` below), even
    // with nothing to run — so the set lives above the output loop:
    // output failures mark it too.
    let mut failed: HashSet<String> = HashSet::new();

    for call in calls {
        match resolve_tool(config, &call.name) {
            ToolKind::Output => {
                // `early` + a final result: later output calls stub
                // out before validation (`_agent_graph.py`
                // output-loop elif) — no failed-mark, no increment,
                // no raise.
                if final_result.is_some() {
                    output_parts.push(RequestPart::ToolReturn {
                        name: call.name.clone(),
                        content: Value::String(
                            "Output tool not used - a final result was already processed."
                                .to_owned(),
                        ),
                        call_id: call.call_id.clone(),
                    });
                    continue;
                }
                // Output calls validate in order; the first success
                // ends the turn. Failures check the consecutive
                // per-name count first (`tool_manager.py:403-408`):
                // at the max the output node relabels the exhaustion
                // to the global message (`_agent_graph.py:1598-1603`)
                // instead of the per-tool text. Otherwise the retry
                // stands and the budget only increments, unchecked
                // (`:1624`) — `consume_output_retry` never runs here.
                match validate_output_call(&call.args) {
                    Err(details) => {
                        if state.retries.get(OUTPUT_TOOL_NAME).copied().unwrap_or(0)
                            == config.max_output_retries
                        {
                            if let Some(message) = state.check_incomplete_tool_call() {
                                return Err(LoopError::failed(message));
                            }
                            return Err(LoopError::failed(format!(
                                "Exceeded maximum output retries ({})",
                                config.max_output_retries
                            )));
                        }
                        failed.insert(OUTPUT_TOOL_NAME.to_owned());
                        output_parts.push(RequestPart::Retry {
                            tool_name: Some(call.name.clone()),
                            content: RetryContent::Errors(details),
                            call_id: call.call_id.clone(),
                        });
                        state.output_retries_used += 1;
                    }
                    Ok(output) => {
                        output_parts.push(RequestPart::ToolReturn {
                            name: call.name.clone(),
                            content: Value::String("Final result processed.".to_owned()),
                            call_id: call.call_id.clone(),
                        });
                        final_result = Some(output);
                    }
                }
            }
            ToolKind::Function | ToolKind::Mcp => function_calls.push(call),
            ToolKind::Unknown => unknown_calls.push(call),
        }
    }

    // With `early` + a final result the function calls stub out
    // (uncounted, unexecuted); unknown calls still process.
    let mut calls_to_run: Vec<&ToolCall> = Vec::new();
    if final_result.is_some() {
        for call in function_calls {
            output_parts.push(RequestPart::ToolReturn {
                name: call.name.clone(),
                content: Value::String(
                    "Tool not executed - a final result was already processed.".to_owned(),
                ),
                call_id: call.call_id.clone(),
            });
        }
    } else {
        calls_to_run.extend(function_calls);
    }
    calls_to_run.extend(unknown_calls);

    // The retry map rebuilds every step, even with nothing to run
    // (an output-only turn resets every count but the marked one).
    if !calls_to_run.is_empty() {
        // The usage gate runs over the projected count (current +
        // pending, unknowns included).
        let projected = state.usage.tool_calls + calls_to_run.len() as i64;
        super::usage::check_before_tool_call(config.limits, projected)?;
        // Validation runs sequentially in call order; the first
        // exhaustion raises before anything executes.
        let mut validated: Vec<ValidatedCall> = Vec::with_capacity(calls_to_run.len());
        for call in &calls_to_run {
            match validate_function_call(config, state, call) {
                ValidationResult::Valid(valid) => validated.push(valid),
                ValidationResult::Retry(failure) => {
                    output_parts.push(failure.part);
                    failed.insert(failure.tool_name);
                }
                ValidationResult::Exhausted(error) => return Err(error),
            }
        }
        // The valid calls execute concurrently; results append in
        // call order; usage counts successes.
        let mut successes = 0;
        for part in execute_validated(config, executor, state, validated).await? {
            if matches!(part, RequestPart::ToolReturn { .. }) {
                // Stubs never occur here (only above); every return
                // is a counted success.
                successes += 1;
            }
            // Execution-time retries mark their tool failed.
            if let RequestPart::Retry {
                tool_name: Some(name),
                ..
            } = &part
            {
                failed.insert(name.clone());
            }
            output_parts.push(part);
        }
        state.usage.tool_calls += successes;
    }
    state.retries = for_run_step(&state.retries, &failed);
    Ok(TurnOutcome {
        parts: output_parts,
        final_result,
    })
}

/// Resolve one call name: `final_result` is the output tool, granted
/// GitHub names are MCP, known function names are function tools,
/// anything else is unknown.
fn resolve_tool(config: &LoopConfig<'_>, name: &str) -> ToolKind {
    if name == OUTPUT_TOOL_NAME {
        ToolKind::Output
    } else if config.mcp_names.contains(name) {
        ToolKind::Mcp
    } else if config.function_schemas.contains_key(name) {
        ToolKind::Function
    } else {
        ToolKind::Unknown
    }
}

/// Validate one output-tool call's arguments
/// (`execute_output_tool_call` validation): dicts validate as
/// objects, strings fence-strip then `validate_json(data or '{}')`.
fn validate_output_call(
    args: &ToolArgs,
) -> Result<pidash_types::dispatch::CloudAgentOutput, Vec<ValidationDetail>> {
    match args {
        ToolArgs::Dict(map) => super::validation::validate_output(&Value::Object(map.clone())),
        ToolArgs::Raw(raw) => validate_output_text(raw),
    }
}

/// One validated function call: the name, id, kind, and validated
/// arguments ready to execute.
struct ValidatedCall {
    name: String,
    call_id: String,
    kind: ToolKind,
    function_args: Option<ValidatedArgs>,
    mcp_args: Map<String, Value>,
}

/// A validation failure: the retry part plus the failed tool name
/// for the retry counters.
struct ValidationFailure {
    part: RequestPart,
    tool_name: String,
}

/// What one call's validation yields: valid, a retry (the caller
/// appends the part and marks the name failed), or an exhaustion
/// that aborts the turn.
enum ValidationResult {
    Valid(ValidatedCall),
    Retry(ValidationFailure),
    Exhausted(LoopError),
}

/// Validate one function/MCP/unknown call (`validate_tool_call` /
/// `ToolManager`): unknown names retry against the sorted available
/// list; function args lax-validate; MCP args must parse as a dict
/// (the `TOOL_SCHEMA_VALIDATOR` dict check — the jsonschema pass
/// runs server-side at execution).
fn validate_function_call(
    config: &LoopConfig<'_>,
    state: &LoopState,
    call: &ToolCall,
) -> ValidationResult {
    let fail = |state: &LoopState,
                name: &str,
                max_retries: u64,
                content: RetryContent,
                call_id: &str|
     -> ValidationResult {
        // The exhaustion check raises; otherwise the retry part
        // stands and the caller marks the name failed.
        if let Err(error) = state.check_max_retries(name, max_retries) {
            return ValidationResult::Exhausted(error);
        }
        ValidationResult::Retry(ValidationFailure {
            part: RequestPart::Retry {
                tool_name: Some(name.to_owned()),
                content,
                call_id: call_id.to_owned(),
            },
            tool_name: name.to_owned(),
        })
    };
    match resolve_tool(config, &call.name) {
        ToolKind::Output => unreachable!("output calls dispatch separately"),
        ToolKind::Unknown => {
            let mut available: Vec<&str> = config
                .wire_tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect();
            available.sort_unstable();
            let message = super::validation::unknown_tool_content(&call.name, &available);
            fail(
                state,
                &call.name,
                // Unknown names budget against `default_max_retries`
                // (`tool_manager.py:407`), like function tools.
                config.max_tool_retries,
                RetryContent::Text(message),
                &call.call_id,
            )
        }
        ToolKind::Function => {
            let schema = config.function_schemas.get(&call.name).unwrap();
            let max_retries = config.max_tool_retries;
            let validated = match &call.args {
                ToolArgs::Dict(map) => {
                    super::validation::validate_args_value(schema, &Value::Object(map.clone()))
                }
                ToolArgs::Raw(raw) => {
                    // `validator.validate_json(raw_args or '{}')`.
                    let payload = if raw.is_empty() { "{}" } else { raw };
                    super::validation::validate_args_text(schema, payload)
                }
            };
            match validated {
                Ok(args) => ValidationResult::Valid(ValidatedCall {
                    name: call.name.clone(),
                    call_id: call.call_id.clone(),
                    kind: ToolKind::Function,
                    function_args: Some(args),
                    mcp_args: Map::new(),
                }),
                Err(details) => fail(
                    state,
                    &call.name,
                    max_retries,
                    RetryContent::Errors(details),
                    &call.call_id,
                ),
            }
        }
        ToolKind::Mcp => {
            let parsed = match &call.args {
                ToolArgs::Dict(map) => Some(map.clone()),
                ToolArgs::Raw(raw) => {
                    let payload = if raw.is_empty() { "{}" } else { raw };
                    match serde_json::from_str::<Value>(payload) {
                        Ok(Value::Object(map)) => Some(map),
                        Ok(_) => {
                            return fail(
                                state,
                                &call.name,
                                config.mcp_max_retries,
                                RetryContent::Errors(vec![ValidationDetail {
                                    error_type: "dict_type",
                                    loc: Vec::new(),
                                    msg: "Input should be a valid dictionary".to_owned(),
                                    input: Value::String(raw.clone()),
                                }]),
                                &call.call_id,
                            );
                        }
                        Err(error) => {
                            return fail(
                                state,
                                &call.name,
                                config.mcp_max_retries,
                                RetryContent::Errors(vec![ValidationDetail {
                                    error_type: "json_invalid",
                                    loc: Vec::new(),
                                    msg: format!("Invalid JSON: {error}"),
                                    input: Value::String(payload.to_owned()),
                                }]),
                                &call.call_id,
                            );
                        }
                    }
                }
            };
            ValidationResult::Valid(ValidatedCall {
                name: call.name.clone(),
                call_id: call.call_id.clone(),
                kind: ToolKind::Mcp,
                function_args: None,
                mcp_args: parsed.unwrap_or_default(),
            })
        }
    }
}

/// Execute the validated batch concurrently (`_call_tools`): all
/// calls poll together, results append in call order, and the
/// first-completed error wins while siblings are dropped
/// (`FIRST_COMPLETED` + `cancel_and_drain`).
async fn execute_validated<E: ToolExecutor>(
    config: &LoopConfig<'_>,
    executor: &E,
    state: &LoopState,
    validated: Vec<ValidatedCall>,
) -> Result<Vec<RequestPart>, LoopError> {
    use futures_util::StreamExt as _;
    if validated.is_empty() {
        return Ok(Vec::new());
    }
    let mut pending: futures_util::stream::FuturesUnordered<_> = validated
        .into_iter()
        .enumerate()
        .map(|(index, call)| async move {
            let outcome = execute_one(config, executor, state, call).await;
            (index, outcome)
        })
        .collect();
    // Drain in finish order; the first error aborts the rest, like
    // `cancel_and_drain` (dropping the stream drops the siblings).
    let mut ordered: Vec<(usize, Result<RequestPart, LoopError>)> = Vec::new();
    while let Some((index, outcome)) = pending.next().await {
        match outcome {
            Ok(part) => ordered.push((index, Ok(part))),
            Err(error) => return Err(error),
        }
    }
    ordered.sort_by_key(|(index, _)| *index);
    let mut parts = Vec::with_capacity(ordered.len());
    for (_, outcome) in ordered {
        parts.push(outcome?);
    }
    Ok(parts)
}

/// Execute one validated call (`_call_tool` / `_raw_execute`):
/// `ModelRetry` retries against the tool's budget, anything else
/// propagates raw, successes return their content. The caller
/// derives the failed names from the retry parts.
async fn execute_one<E: ToolExecutor>(
    config: &LoopConfig<'_>,
    executor: &E,
    state: &LoopState,
    call: ValidatedCall,
) -> Result<RequestPart, LoopError> {
    let max_retries = match call.kind {
        ToolKind::Function => config.max_tool_retries,
        ToolKind::Mcp => config.mcp_max_retries,
        ToolKind::Output | ToolKind::Unknown => unreachable!("validated calls only"),
    };
    let outcome = match call.kind {
        ToolKind::Function => {
            // No timeout: sync tools run to completion (see the
            // module docs).
            executor
                .call_function(&call.name, call.function_args.unwrap(), &call.call_id)
                .await
        }
        ToolKind::Mcp => execute_mcp(config, executor, &call).await?,
        ToolKind::Output | ToolKind::Unknown => unreachable!("validated calls only"),
    };
    match outcome {
        ToolOutcome::Ok(content) => Ok(RequestPart::ToolReturn {
            name: call.name,
            content,
            call_id: call.call_id,
        }),
        ToolOutcome::ModelRetry(message) => {
            state.check_max_retries(&call.name, max_retries)?;
            Ok(RequestPart::Retry {
                tool_name: Some(call.name.clone()),
                content: RetryContent::Text(message),
                call_id: call.call_id,
            })
        }
        ToolOutcome::Fatal(message) => Err(LoopError::failed(message)),
        ToolOutcome::Timeout => Err(LoopError::Timeout),
    }
}

/// The in-process FastMCP leg (`build_github_toolset`): the MCP
/// schema jsonschema-validates the arguments (failures are fatal
/// `ToolError`s, never retries), the sync twin runs under the real
/// timeout with the call id threaded through, and every twin error
/// masks to `Error calling tool 'name'`
/// (`mask_error_details=True`).
async fn execute_mcp<E: ToolExecutor>(
    config: &LoopConfig<'_>,
    executor: &E,
    call: &ValidatedCall,
) -> Result<ToolOutcome, LoopError> {
    let schema = config.mcp_schemas.get(&call.name).unwrap();
    if let Err(message) = jsonschema_validate(&schema.parameters, &call.mcp_args) {
        // `Input validation error: ...` (`mcp/server/lowlevel/
        // server.py:538`): an `isError` result the client raises as
        // `ToolError`, re-raised raw (`tool_error_behavior='error'`).
        // Unmasked: validation runs before the tool body.
        return Ok(ToolOutcome::Fatal(format!(
            "Input validation error: {message}"
        )));
    }
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(config.mcp_timeout_secs.max(1)),
        executor.call_mcp(&call.name, call.mcp_args.clone(), &call.call_id),
    )
    .await;
    match outcome {
        Err(_) => Ok(ToolOutcome::Timeout),
        Ok(ToolOutcome::Ok(value)) => Ok(ToolOutcome::Ok(value)),
        // The twin's `ModelRetry` is not special on this path: any
        // twin failure masks the same way.
        Ok(ToolOutcome::ModelRetry(_)) | Ok(ToolOutcome::Fatal(_)) | Ok(ToolOutcome::Timeout) => {
            Ok(ToolOutcome::Fatal(format!(
                "Error calling tool '{}'",
                call.name
            )))
        }
    }
}

/// `for_run_step`: rebuild the failure counts for the next step
/// (`tool_manager.py:122-140`): only tools that failed this step
/// carry over, bumped by one — everything else resets. Runs every
/// step, including steps with no tool failures.
fn for_run_step(retries: &HashMap<String, u64>, failed: &HashSet<String>) -> HashMap<String, u64> {
    failed
        .iter()
        .map(|name| (name.clone(), retries.get(name).copied().unwrap_or(0) + 1))
        .collect()
}

// ---------------------------------------------------------------------------
// The MCP jsonschema pass
// ---------------------------------------------------------------------------

/// Validate MCP arguments against the tool's input schema
/// (`jsonschema.validate`, `mcp/server/lowlevel/server.py:530-538`)
/// for the shapes these schemas use (`type`, `required`,
/// `properties`, `additionalProperties: false`, `default`-less):
/// the first failure message, in `jsonschema` wording (`'xx' is not
/// of type 'integer'`, `'path' is a required property`,
/// `Additional properties are not allowed (...)`). Keyword order is
/// `required` > `additionalProperties` > `type`.
fn jsonschema_validate(schema: &Value, args: &Map<String, Value>) -> Result<(), String> {
    let Value::Object(schema) = schema else {
        return Ok(());
    };
    if let Value::Array(required) = schema.get("required").unwrap_or(&Value::Null) {
        for name in required.iter().filter_map(Value::as_str) {
            if !args.contains_key(name) {
                return Err(format!("'{name}' is a required property"));
            }
        }
    }
    if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
        let known = schema.get("properties").and_then(Value::as_object);
        // `extras_msg(sorted(extras))`: one quoted name + `was`,
        // several sorted + `were` (`_utils.py:87-92`).
        let mut extras: Vec<&str> = args
            .keys()
            .filter(|name| !known.is_some_and(|known| known.contains_key(*name)))
            .map(String::as_str)
            .collect();
        extras.sort_unstable();
        if extras.len() == 1 {
            return Err(format!(
                "Additional properties are not allowed ('{}' was unexpected)",
                extras[0]
            ));
        }
        if extras.len() > 1 {
            let joined = extras
                .iter()
                .map(|name| format!("'{name}'"))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "Additional properties are not allowed ({joined} were unexpected)"
            ));
        }
    }
    if let Value::Object(properties) = schema.get("properties").unwrap_or(&Value::Null) {
        // Properties validate in schema order (the `jsonschema`
        // validator walks `properties` in order).
        for (name, subschema) in properties {
            let Some(value) = args.get(name) else {
                continue;
            };
            jsonschema_check_type(subschema, value)?;
        }
    }
    Ok(())
}

/// The `type` check with `jsonschema` messages: values Python-`repr`'d
/// (`'xx' is not of type 'integer'`).
fn jsonschema_check_type(subschema: &Value, value: &Value) -> Result<(), String> {
    let Value::Object(schema) = subschema else {
        return Ok(());
    };
    let Some(expected) = schema.get("type").and_then(Value::as_str) else {
        return Ok(());
    };
    let matches = match expected {
        "string" => value.is_string(),
        // `jsonschema` booleans are not integers (explicit carve-out).
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        "null" => value.is_null(),
        _ => true,
    };
    if matches {
        return Ok(());
    }
    Err(format!(
        "{} is not of type '{}'",
        super::provider::py_repr(value),
        expected
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oss_worker::model_runtime::schemas::Golden;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// One scripted mock reply.
    #[derive(Clone)]
    enum MockReply {
        Json(Value),
        Status(u16, Vec<u8>),
    }

    #[derive(Clone)]
    struct MockState {
        replies: Arc<Mutex<VecDeque<MockReply>>>,
        last: Arc<Mutex<MockReply>>,
        requests: Arc<Mutex<Vec<Value>>>,
    }

    async fn handler(State(state): State<MockState>, body: String) -> impl IntoResponse {
        state
            .requests
            .lock()
            .unwrap()
            .push(serde_json::from_str(&body).unwrap_or(Value::Null));
        let reply = state
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| state.last.lock().unwrap().clone());
        *state.last.lock().unwrap() = reply.clone();
        match reply {
            MockReply::Json(value) => (StatusCode::OK, value.to_string()).into_response(),
            MockReply::Status(status, bytes) => (
                StatusCode::from_u16(status).unwrap(),
                String::from_utf8_lossy(&bytes).into_owned(),
            )
                .into_response(),
        }
    }

    async fn mock_server(replies: Vec<MockReply>) -> (String, MockState) {
        let state = MockState {
            last: Arc::new(Mutex::new(replies[0].clone())),
            replies: Arc::new(Mutex::new(replies.into())),
            requests: Arc::new(Mutex::new(Vec::new())),
        };
        let app = axum::Router::new()
            .route("/v1/chat/completions", axum::routing::post(handler))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}/v1"), state)
    }

    fn chat_reply(message: Value, finish: &str) -> MockReply {
        MockReply::Json(serde_json::json!({
            "id": "c1", "object": "chat.completion", "created": 1, "model": "m",
            "choices": [{"index": 0, "message": message, "finish_reason": finish}],
            "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8},
        }))
    }

    fn text_reply(text: &str) -> MockReply {
        chat_reply(
            serde_json::json!({"role": "assistant", "content": text}),
            "stop",
        )
    }

    fn tool_reply(calls: Vec<(&str, &str, &str)>) -> MockReply {
        let tool_calls: Vec<Value> = calls
            .into_iter()
            .map(|(id, name, arguments)| {
                serde_json::json!({
                    "id": id, "type": "function",
                    "function": {"name": name, "arguments": arguments},
                })
            })
            .collect();
        chat_reply(
            serde_json::json!({"role": "assistant", "content": null, "tool_calls": tool_calls}),
            "tool_calls",
        )
    }

    fn done_text(outcome: &str, summary: &str) -> String {
        format!(r#"{{"outcome": "{outcome}", "summary": {summary:?}}}"#)
    }

    /// A fake executor with scripted per-tool behaviors.
    enum Behavior {
        Add,
        Fatal(String),
        RetryThenOk(Value),
        Slow(u64, Value),
    }

    struct FakeExecutor {
        behaviors: HashMap<String, Behavior>,
        seen: Mutex<Vec<String>>,
        retried: Mutex<HashSet<String>>,
    }

    impl FakeExecutor {
        fn named(pairs: Vec<(&str, Behavior)>) -> Self {
            FakeExecutor {
                behaviors: pairs
                    .into_iter()
                    .map(|(name, behavior)| (name.to_owned(), behavior))
                    .collect(),
                seen: Mutex::new(Vec::new()),
                retried: Mutex::new(HashSet::new()),
            }
        }
    }

    impl ToolExecutor for FakeExecutor {
        async fn call_function(
            &self,
            name: &str,
            args: ValidatedArgs,
            _call_id: &str,
        ) -> ToolOutcome {
            self.seen.lock().unwrap().push(name.to_owned());
            match self.behaviors.get(name) {
                Some(Behavior::Add) => {
                    let sum = args.get_int("a").unwrap_or(0) + args.get_int("b").unwrap_or(0);
                    ToolOutcome::Ok(Value::Number(sum.into()))
                }
                Some(Behavior::Fatal(message)) => ToolOutcome::Fatal(message.clone()),
                Some(Behavior::RetryThenOk(value)) => {
                    let mut retried = self.retried.lock().unwrap();
                    if retried.insert(name.to_owned()) {
                        ToolOutcome::ModelRetry("try again".to_owned())
                    } else {
                        ToolOutcome::Ok(value.clone())
                    }
                }
                Some(Behavior::Slow(secs, value)) => {
                    tokio::time::sleep(Duration::from_secs(*secs)).await;
                    ToolOutcome::Ok(value.clone())
                }
                None => ToolOutcome::Fatal(format!("unexpected tool {name}")),
            }
        }

        async fn call_mcp(
            &self,
            name: &str,
            args: Map<String, Value>,
            _call_id: &str,
        ) -> ToolOutcome {
            self.seen.lock().unwrap().push(name.to_owned());
            match self.behaviors.get(name) {
                Some(Behavior::Fatal(message)) => ToolOutcome::Fatal(message.clone()),
                Some(Behavior::Slow(secs, value)) => {
                    tokio::time::sleep(Duration::from_secs(*secs)).await;
                    ToolOutcome::Ok(value.clone())
                }
                _ => ToolOutcome::Ok(Value::Object(args)),
            }
        }
    }

    fn add_schema() -> ToolSchema {
        ToolSchema {
            name: "add".to_owned(),
            description: "Add.".to_owned(),
            parameters: serde_json::from_str(
                r#"{"additionalProperties":false,"properties":{"a":{"type":"integer"},"b":{"type":"integer"}},"required":["a","b"],"type":"object"}"#,
            )
            .unwrap(),
        }
    }

    fn output_wire_tool() -> WireTool {
        let raw: Value = serde_json::from_str(
            r#"{"properties":{"sum":{"type":"integer"}},"required":["sum"],"type":"object"}"#,
        )
        .unwrap();
        let (parameters, strict) =
            crate::oss_worker::model_runtime::provider::transform_openai_schema(&raw);
        WireTool {
            name: OUTPUT_TOOL_NAME.to_owned(),
            description: "The final response which ends this conversation".to_owned(),
            parameters,
            strict: strict.then_some(true),
        }
    }

    fn add_wire_tool() -> WireTool {
        let (parameters, strict) =
            crate::oss_worker::model_runtime::provider::transform_openai_schema(
                &add_schema().parameters,
            );
        WireTool {
            name: "add".to_owned(),
            description: "Add.".to_owned(),
            parameters,
            strict: strict.then_some(true),
        }
    }

    fn no_limits() -> crate::dispatch_execute::UsageLimits {
        crate::dispatch_execute::UsageLimits {
            request_limit: None,
            tool_calls_limit: None,
            input_tokens_limit: None,
            output_tokens_limit: None,
            total_tokens_limit: None,
        }
    }

    struct Harness {
        lane: Lane,
        client: reqwest::Client,
        table: ProviderTable,
        wire_tools: Vec<WireTool>,
        function_schemas: HashMap<String, ToolSchema>,
        mcp_schemas: HashMap<String, ToolSchema>,
        mcp_names: HashSet<String>,
        limits: crate::dispatch_execute::UsageLimits,
        mcp_timeout_secs: u64,
        state: MockState,
    }

    impl Harness {
        fn config(&self) -> LoopConfig<'_> {
            LoopConfig {
                lane: &self.lane,
                client: &self.client,
                extract: &self.table,
                instructions: "",
                prompt: "go",
                wire_tools: &self.wire_tools,
                function_schemas: &self.function_schemas,
                mcp_schemas: &self.mcp_schemas,
                mcp_names: &self.mcp_names,
                max_tool_retries: 2,
                max_output_retries: 2,
                mcp_max_retries: 2,
                limits: &self.limits,
                max_tokens: 4096,
                timeout_secs: 60,
                mcp_timeout_secs: self.mcp_timeout_secs,
            }
        }
    }

    async fn harness(
        replies: Vec<MockReply>,
        tools: Vec<WireTool>,
        functions: Vec<ToolSchema>,
        mcp: Vec<ToolSchema>,
    ) -> Harness {
        let (base_url, state) = mock_server(replies).await;
        let golden = Golden::load().expect("golden parses");
        let function_schemas: HashMap<String, ToolSchema> = functions
            .into_iter()
            .map(|schema| (schema.name.clone(), schema))
            .collect();
        let mcp_schemas: HashMap<String, ToolSchema> = mcp
            .into_iter()
            .map(|schema| (schema.name.clone(), schema))
            .collect();
        let mcp_names = mcp_schemas.keys().cloned().collect();
        Harness {
            lane: Lane::OpenAICompatible {
                model: "m".to_owned(),
                base_url,
                api_key: "k".to_owned(),
            },
            client: reqwest::Client::new(),
            table: ProviderTable::load(&golden).expect("table compiles"),
            wire_tools: tools,
            function_schemas,
            mcp_schemas,
            mcp_names,
            limits: no_limits(),
            mcp_timeout_secs: 20,
            state,
        }
    }

    fn failed_message(error: LoopError) -> String {
        match error {
            LoopError::Failed(message) => message,
            LoopError::UsageLimit(message) => panic!("expected Failed, got UsageLimit: {message}"),
            LoopError::Timeout => panic!("expected Failed, got Timeout"),
        }
    }

    #[tokio::test]
    async fn two_turn_tool_loop() {
        let board = harness(
            vec![
                tool_reply(vec![("c1", "add", r#"{"a": 2, "b": 3}"#)]),
                text_reply(&done_text("completed", "added")),
            ],
            vec![add_wire_tool(), output_wire_tool()],
            vec![add_schema()],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![("add", Behavior::Add)]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "added");
        assert_eq!(output.usage.requests, 2);
        assert_eq!(output.usage.tool_calls, 1);
        // The tool result replays as compact JSON on turn 2.
        let requests = board.state.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1]["messages"][2]["content"],
            Value::String("5".to_owned())
        );
    }

    #[tokio::test]
    async fn output_retry_then_ok() {
        let board = harness(
            vec![
                text_reply(r#"{"nope": 1}"#),
                text_reply(&done_text("completed", "fixed")),
            ],
            vec![output_wire_tool()],
            vec![],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "fixed");
        assert_eq!(output.usage.requests, 2);
        let requests = board.state.requests.lock().unwrap();
        let retry = requests[1]["messages"][2]["content"].as_str().unwrap();
        assert!(retry.starts_with("2 validation errors:"), "got: {retry}");
        assert!(retry.ends_with("\n\nFix the errors and try again."));
    }

    #[tokio::test]
    async fn output_exhaustion() {
        let board = harness(
            vec![text_reply(r#"{"nope": 1}"#)],
            vec![output_wire_tool()],
            vec![],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let error = failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
        assert_eq!(error, "Exceeded maximum output retries (2)");
        assert_eq!(board.state.requests.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn output_tool_failures_do_not_spend_checked_budget() {
        // `[text-fail, F(out), F(out), valid]` succeeds in 4 turns:
        // the output-tool path increments `output_retries_used`
        // without enforcing it (`_agent_graph.py:1624`); only the
        // consecutive per-name count can end the run there.
        let valid = done_text("completed", "mixed");
        let board = harness(
            vec![
                text_reply(r#"{"nope": 1}"#),
                tool_reply(vec![("c2", OUTPUT_TOOL_NAME, r#"{"nope": 1}"#)]),
                tool_reply(vec![("c3", OUTPUT_TOOL_NAME, r#"{"nope": 1}"#)]),
                text_reply(&valid),
            ],
            vec![output_wire_tool()],
            vec![],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "mixed");
        assert_eq!(board.state.requests.lock().unwrap().len(), 4);
    }

    #[tokio::test]
    async fn output_call_after_final_result_stubs_unvalidated() {
        // `[F(out), F(out), (valid, invalid)]` succeeds: under
        // `early` the output node stubs every output call once a
        // final result exists — before validation, so the per-name
        // count (at max here) never trips
        // (`_agent_graph.py` output-loop elif).
        let valid = done_text("completed", "dup");
        let board = harness(
            vec![
                tool_reply(vec![("c1", OUTPUT_TOOL_NAME, r#"{"nope": 1}"#)]),
                tool_reply(vec![("c2", OUTPUT_TOOL_NAME, r#"{"nope": 1}"#)]),
                tool_reply(vec![
                    ("c3a", OUTPUT_TOOL_NAME, &valid),
                    ("c3b", OUTPUT_TOOL_NAME, r#"{"nope": 1}"#),
                ]),
            ],
            vec![output_wire_tool()],
            vec![],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "dup");
        assert_eq!(board.state.requests.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn consecutive_output_tool_failures_raise_global() {
        // `[F(out) x 3]` raises the relabeled global message: the
        // consecutive per-name check trips and the output node
        // relabels it (`_agent_graph.py:1586-1603`).
        let board = harness(
            vec![tool_reply(vec![("c1", OUTPUT_TOOL_NAME, r#"{"nope": 1}"#)])],
            vec![output_wire_tool()],
            vec![],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let error = failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
        assert_eq!(error, "Exceeded maximum output retries (2)");
        assert_eq!(board.state.requests.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn tool_fatal_propagates() {
        let board = harness(
            vec![tool_reply(vec![("c1", "boom", "{}")])],
            vec![add_wire_tool(), output_wire_tool()],
            vec![ToolSchema {
                name: "boom".to_owned(),
                description: String::new(),
                parameters: serde_json::from_str(
                    r#"{"additionalProperties":false,"properties":{},"type":"object"}"#,
                )
                .unwrap(),
            }],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![("boom", Behavior::Fatal("kaput".to_owned()))]);
        let error = failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
        assert_eq!(error, "kaput");
        assert_eq!(board.state.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unknown_tool_retries_then_ok() {
        let board = harness(
            vec![
                tool_reply(vec![("c9", "nope", "{}")]),
                text_reply(&done_text("noop", "recovered")),
            ],
            vec![add_wire_tool(), output_wire_tool()],
            vec![add_schema()],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![("add", Behavior::Add)]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "recovered");
        let requests = board.state.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1]["messages"][2]["content"],
            Value::String(
                "Unknown tool name: 'nope'. Available tools: 'add', 'final_result'\n\nFix the errors and try again."
                    .to_owned()
            )
        );
    }

    #[tokio::test]
    async fn unknown_tool_exhaustion() {
        let board = harness(
            vec![tool_reply(vec![("c9", "nope", "{}")])],
            vec![add_wire_tool(), output_wire_tool()],
            vec![add_schema()],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let error = failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
        assert_eq!(error, "Tool 'nope' exceeded max retries count of 2");
        assert_eq!(board.state.requests.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn request_limit() {
        let mut board = harness(
            vec![
                tool_reply(vec![("c1", "add", r#"{"a": 1, "b": 2}"#)]),
                text_reply(&done_text("completed", "x")),
            ],
            vec![add_wire_tool(), output_wire_tool()],
            vec![add_schema()],
            vec![],
        )
        .await;
        board.limits.request_limit = Some(1);
        let executor = FakeExecutor::named(vec![("add", Behavior::Add)]);
        match run_loop(&board.config(), &executor).await.unwrap_err() {
            LoopError::UsageLimit(message) => {
                assert_eq!(
                    message,
                    "The next request would exceed the request_limit of 1"
                );
            }
            other => panic!("expected UsageLimit, got {}", failed_message(other)),
        }
        assert_eq!(board.state.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn tool_calls_limit() {
        let mut board = harness(
            vec![tool_reply(vec![("c1", "add", r#"{"a": 1, "b": 2}"#)])],
            vec![add_wire_tool(), output_wire_tool()],
            vec![add_schema()],
            vec![],
        )
        .await;
        board.limits.tool_calls_limit = Some(0);
        let executor = FakeExecutor::named(vec![("add", Behavior::Add)]);
        match run_loop(&board.config(), &executor).await.unwrap_err() {
            LoopError::UsageLimit(message) => {
                assert_eq!(
                    message,
                    "The next tool call(s) would exceed the tool_calls_limit of 0 (tool_calls=1)."
                );
            }
            other => panic!("expected UsageLimit, got {}", failed_message(other)),
        }
    }

    #[tokio::test]
    async fn http_retry_recovers() {
        let board = harness(
            vec![
                MockReply::Status(500, br#"{"error": "boom"}"#.to_vec()),
                text_reply(&done_text("completed", "after retry")),
            ],
            vec![output_wire_tool()],
            vec![],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "after retry");
        assert_eq!(output.usage.requests, 1);
        assert_eq!(board.state.requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn multi_tool_turn() {
        let board = harness(
            vec![
                tool_reply(vec![
                    ("c1", "add", r#"{"a": 1, "b": 2}"#),
                    ("c2", "add", r#"{"a": 3, "b": 4}"#),
                ]),
                text_reply(&done_text("completed", "both")),
            ],
            vec![add_wire_tool(), output_wire_tool()],
            vec![add_schema()],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![("add", Behavior::Add)]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.usage.tool_calls, 2);
        let requests = board.state.requests.lock().unwrap();
        assert_eq!(
            requests[1]["messages"][2]["content"],
            Value::String("3".to_owned())
        );
        assert_eq!(
            requests[1]["messages"][3]["content"],
            Value::String("7".to_owned())
        );
    }

    #[tokio::test]
    async fn early_end_stubs_function_calls() {
        let board = harness(
            vec![tool_reply(vec![
                ("c1", "final_result", &done_text("completed", "early")),
                ("c2", "add", r#"{"a": 1, "b": 2}"#),
            ])],
            vec![add_wire_tool(), output_wire_tool()],
            vec![add_schema()],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![("add", Behavior::Add)]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "early");
        assert_eq!(output.usage.tool_calls, 0);
        assert!(executor.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn text_beside_tools_is_ignored() {
        let board = harness(
            vec![
                chat_reply(
                    serde_json::json!({
                        "role": "assistant",
                        "content": &done_text("completed", "ignored"),
                        "tool_calls": [{
                            "id": "c1", "type": "function",
                            "function": {"name": "add", "arguments": r#"{"a": 1, "b": 2}"#},
                        }],
                    }),
                    "tool_calls",
                ),
                text_reply(&done_text("completed", "kept")),
            ],
            vec![add_wire_tool(), output_wire_tool()],
            vec![add_schema()],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![("add", Behavior::Add)]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "kept");
        assert_eq!(executor.seen.lock().unwrap().as_slice(), ["add"]);
    }

    #[tokio::test]
    async fn refusal_is_content_filter() {
        let board = harness(
            vec![chat_reply(
                serde_json::json!({"role": "assistant", "content": null, "refusal": "nope"}),
                "stop",
            )],
            vec![output_wire_tool()],
            vec![],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let error = failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
        let (head, body) = error.split_once(", body:\n").expect("filter body");
        assert_eq!(head, "Content filter triggered. Refusal: 'nope'");
        // The body mirrors `ModelMessagesTypeAdapter.dump_json`
        // exactly: dataclass key order, the actual response usage,
        // nulls included.
        let parsed: Value = serde_json::from_str(body).expect("body is JSON");
        let message = parsed[0].as_object().expect("message object");
        assert_eq!(
            message.keys().map(String::as_str).collect::<Vec<_>>(),
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
                "state",
            ]
            .as_slice()
        );
        assert_eq!(message["kind"], Value::String("response".to_owned()));
        assert_eq!(
            message["usage"],
            serde_json::json!({
                "input_tokens": 5,
                "cache_write_tokens": 0,
                "cache_read_tokens": 0,
                "output_tokens": 3,
                "input_audio_tokens": 0,
                "cache_audio_read_tokens": 0,
                "output_audio_tokens": 0,
                "details": {},
            })
        );
        assert_eq!(
            message["usage"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "input_tokens",
                "cache_write_tokens",
                "cache_read_tokens",
                "output_tokens",
                "input_audio_tokens",
                "cache_audio_read_tokens",
                "output_audio_tokens",
                "details",
            ]
            .as_slice()
        );
        assert_eq!(message["model_name"], Value::String("m".to_owned()));
        assert!(message["timestamp"].as_str().unwrap().ends_with('Z'));
        assert_eq!(message["provider_name"], Value::String("openai".to_owned()));
        assert!(message["provider_url"]
            .as_str()
            .unwrap()
            .starts_with("http://127.0.0.1:"));
        assert_eq!(
            message["provider_details"],
            serde_json::json!({"refusal": "nope", "timestamp": "1970-01-01T00:00:01Z"})
        );
        assert_eq!(
            message["provider_response_id"],
            Value::String("c1".to_owned())
        );
        assert_eq!(
            message["finish_reason"],
            Value::String("content_filter".to_owned())
        );
        assert!(uuid::Uuid::parse_str(message["run_id"].as_str().unwrap()).is_ok());
        assert!(uuid::Uuid::parse_str(message["conversation_id"].as_str().unwrap()).is_ok());
        assert_eq!(message["metadata"], Value::Null);
        assert_eq!(message["state"], Value::String("complete".to_owned()));
    }

    #[tokio::test]
    async fn refusal_head_uses_py_repr() {
        // `Refusal: {refusal!r}` (`_agent_graph.py:1128`): a refusal
        // holding a single quote renders double-quoted.
        let board = harness(
            vec![chat_reply(
                serde_json::json!({"role": "assistant", "content": null, "refusal": "o'clock"}),
                "stop",
            )],
            vec![output_wire_tool()],
            vec![],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let error = failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
        let (head, _) = error.split_once(", body:\n").expect("filter body");
        assert_eq!(head, "Content filter triggered. Refusal: \"o'clock\"");
    }

    #[test]
    fn textless_retry_text_names_both_alternatives() {
        // `Please {" or ".join(alternatives)}.` with the
        // output-toolset alternative plus `return text`, since the
        // (present) text processor got no text
        // (`_agent_graph.py:1216-1246`).
        let content = RetryContent::Text(TEXTLESS_RETRY_TEXT.to_owned());
        assert_eq!(
            crate::oss_worker::model_runtime::provider::retry_message(&content, None),
            "Validation feedback:\nPlease return text or include your response in a tool call.\n\nFix the errors and try again."
        );
    }

    #[tokio::test]
    async fn length_empty_names_max_tokens() {
        let board = harness(
            vec![chat_reply(
                serde_json::json!({"role": "assistant", "content": null}),
                "length",
            )],
            vec![output_wire_tool()],
            vec![],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let error = failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
        assert!(
            error.starts_with("Model token limit (4096) exceeded"),
            "got: {error}"
        );
    }

    #[tokio::test]
    async fn length_with_text_retries_output() {
        let board = harness(
            vec![
                chat_reply(
                    serde_json::json!({"role": "assistant", "content": r#"{"outcome": "#}),
                    "length",
                ),
                text_reply(&done_text("completed", "recovered")),
            ],
            vec![output_wire_tool()],
            vec![],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "recovered");
    }

    #[tokio::test]
    async fn empty_response_resubmits() {
        let board = harness(
            vec![
                chat_reply(
                    serde_json::json!({"role": "assistant", "content": null}),
                    "stop",
                ),
                text_reply(&done_text("completed", "second try")),
            ],
            vec![output_wire_tool()],
            vec![],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "second try");
        assert_eq!(board.state.requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn fenced_output_parses() {
        let board = harness(
            vec![text_reply(
                "Here you go:\n```json\n{\"outcome\": \"completed\", \"summary\": \"fenced\"}\n```",
            )],
            vec![output_wire_tool()],
            vec![],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "fenced");
    }

    #[tokio::test]
    async fn multiline_fenced_output_parses_first_try() {
        // `re.DOTALL` (`_utils.py:1091`): the `{...}` group spans
        // newlines, so pretty-printed fenced JSON validates without
        // a retry.
        let board = harness(
            vec![text_reply(
                "Here you go:\n```json\n{\n\"outcome\": \"completed\",\n\"summary\": \"pretty\"\n}\n```",
            )],
            vec![output_wire_tool()],
            vec![],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "pretty");
        assert_eq!(board.state.requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn recover_text_skips_empty_responses() {
        // `_recover_text_from_message_history`: the scan never stops
        // early — an empty response between the text turn and the
        // recovery turn does not hide the older text.
        fn response(parts: Vec<ResponsePart>) -> HistoryMessage {
            HistoryMessage::Response {
                parts,
                usage: RequestUsage::zeros(),
                model_name: "m".to_owned(),
                finish: None,
                provider_details: None,
                response_id: None,
            }
        }
        let history = vec![
            HistoryMessage::Request {
                parts: vec![RequestPart::UserPrompt("go".to_owned())],
            },
            response(vec![ResponsePart::Text("stale but failed".to_owned())]),
            HistoryMessage::Request {
                parts: vec![RequestPart::Retry {
                    tool_name: None,
                    content: RetryContent::Text("retry".to_owned()),
                    call_id: "c0".to_owned(),
                }],
            },
            // The empty arm's push plus its resubmit.
            response(Vec::new()),
            HistoryMessage::Request { parts: Vec::new() },
            // The current textless turn (already in history).
            response(vec![ResponsePart::Text(String::new())]),
        ];
        assert_eq!(recover_text(&history).as_deref(), Some("stale but failed"));
        assert_eq!(recover_text(&history[..1]), None);
    }

    #[tokio::test]
    async fn tool_arg_exhaustion() {
        let board = harness(
            vec![tool_reply(vec![("c1", "add", r#"{"a": "xx"}"#)])],
            vec![add_wire_tool(), output_wire_tool()],
            vec![add_schema()],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![("add", Behavior::Add)]);
        let error = failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
        assert_eq!(error, "Tool 'add' exceeded max retries count of 2");
        assert_eq!(board.state.requests.lock().unwrap().len(), 3);
        assert!(executor.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn tool_retry_counts_reset_on_success() {
        // Consecutive budgets (`tool_manager.py:122-140`): fail,
        // succeed, fail, fail never exhausts — the success resets
        // the count. (Cumulative counts would exhaust on turn 4.)
        let board = harness(
            vec![
                tool_reply(vec![("c1", "add", r#"{"a": "xx"}"#)]),
                tool_reply(vec![("c2", "add", r#"{"a": 1, "b": 2}"#)]),
                tool_reply(vec![("c3", "add", r#"{"a": "xx"}"#)]),
                tool_reply(vec![("c4", "add", r#"{"a": "xx"}"#)]),
                text_reply(&done_text("completed", "survived")),
            ],
            vec![add_wire_tool(), output_wire_tool()],
            vec![add_schema()],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![("add", Behavior::Add)]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "survived");
        assert_eq!(output.usage.tool_calls, 1);
        assert_eq!(board.state.requests.lock().unwrap().len(), 5);
    }

    #[tokio::test]
    async fn tools_run_concurrently() {
        let board = harness(
            vec![
                tool_reply(vec![("c1", "slow_a", "{}"), ("c2", "slow_b", "{}")]),
                text_reply(&done_text("completed", "fast")),
            ],
            vec![add_wire_tool(), output_wire_tool()],
            vec![
                ToolSchema {
                    name: "slow_a".to_owned(),
                    description: String::new(),
                    parameters: serde_json::from_str(
                        r#"{"additionalProperties":false,"properties":{},"type":"object"}"#,
                    )
                    .unwrap(),
                },
                ToolSchema {
                    name: "slow_b".to_owned(),
                    description: String::new(),
                    parameters: serde_json::from_str(
                        r#"{"additionalProperties":false,"properties":{},"type":"object"}"#,
                    )
                    .unwrap(),
                },
            ],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![
            ("slow_a", Behavior::Slow(1, Value::String("a".to_owned()))),
            ("slow_b", Behavior::Slow(1, Value::String("b".to_owned()))),
        ]);
        let started = std::time::Instant::now();
        run_loop(&board.config(), &executor).await.expect("runs");
        assert!(started.elapsed() < Duration::from_millis(1900));
    }

    fn mcp_add_schema() -> ToolSchema {
        ToolSchema {
            name: "mcp_add".to_owned(),
            description: "Add via MCP.".to_owned(),
            parameters: serde_json::from_str(
                r#"{"additionalProperties":false,"properties":{"a":{"type":"integer"}},"required":["a"],"type":"object"}"#,
            )
            .unwrap(),
        }
    }

    fn mcp_add_wire_tool() -> WireTool {
        let (parameters, _) = crate::oss_worker::model_runtime::provider::transform_openai_schema(
            &mcp_add_schema().parameters,
        );
        WireTool {
            name: "mcp_add".to_owned(),
            description: "Add via MCP.".to_owned(),
            parameters,
            strict: None,
        }
    }

    #[tokio::test]
    async fn mcp_success_echoes_args() {
        let board = harness(
            vec![
                tool_reply(vec![("c1", "mcp_add", r#"{"a": 4}"#)]),
                text_reply(&done_text("completed", "mcp ok")),
            ],
            vec![mcp_add_wire_tool(), output_wire_tool()],
            vec![],
            vec![mcp_add_schema()],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "mcp ok");
        assert_eq!(output.usage.tool_calls, 1);
        let requests = board.state.requests.lock().unwrap();
        assert_eq!(
            requests[1]["messages"][2]["content"],
            Value::String(r#"{"a":4}"#.to_owned())
        );
    }

    #[tokio::test]
    async fn mcp_schema_violation_is_fatal() {
        // `mcpprobe.json` `mcp_badargs`: jsonschema wording, fatal.
        let board = harness(
            vec![tool_reply(vec![("c1", "mcp_add", r#"{"a": "xx"}"#)])],
            vec![mcp_add_wire_tool(), output_wire_tool()],
            vec![],
            vec![mcp_add_schema()],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let error = failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
        assert_eq!(
            error,
            "Input validation error: 'xx' is not of type 'integer'"
        );
        assert!(executor.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn mcp_missing_required_is_fatal() {
        let board = harness(
            vec![tool_reply(vec![("c1", "mcp_add", "{}")])],
            vec![mcp_add_wire_tool(), output_wire_tool()],
            vec![],
            vec![mcp_add_schema()],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let error = failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
        assert_eq!(error, "Input validation error: 'a' is a required property");
    }

    #[tokio::test]
    async fn mcp_extra_arg_is_fatal() {
        // `additionalProperties: false` (`_keywords.py:44-56`): one
        // extra, singular message.
        let board = harness(
            vec![tool_reply(vec![("c1", "mcp_add", r#"{"a": 1, "zzz": 2}"#)])],
            vec![mcp_add_wire_tool(), output_wire_tool()],
            vec![],
            vec![mcp_add_schema()],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let error = failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
        assert_eq!(
            error,
            "Input validation error: Additional properties are not allowed ('zzz' was unexpected)"
        );
        assert!(executor.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn mcp_extra_args_sort_with_plural() {
        // Several extras: one error, sorted names, `were`.
        let board = harness(
            vec![tool_reply(vec![(
                "c1",
                "mcp_add",
                r#"{"a": 1, "zzz": 2, "aaa": 3}"#,
            )])],
            vec![mcp_add_wire_tool(), output_wire_tool()],
            vec![],
            vec![mcp_add_schema()],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let error = failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
        assert_eq!(
            error,
            "Input validation error: Additional properties are not allowed ('aaa', 'zzz' were unexpected)"
        );
    }

    #[tokio::test]
    async fn mcp_keyword_precedence() {
        // `required` beats `additionalProperties` beats `type`.
        for (args, expected) in [
            (
                r#"{"zzz": 1}"#,
                "Input validation error: 'a' is a required property",
            ),
            (
                r#"{"a": "x", "zzz": 1}"#,
                "Input validation error: Additional properties are not allowed ('zzz' was unexpected)",
            ),
        ] {
            let board = harness(
                vec![tool_reply(vec![("c1", "mcp_add", args)])],
                vec![mcp_add_wire_tool(), output_wire_tool()],
                vec![],
                vec![mcp_add_schema()],
            )
            .await;
            let executor = FakeExecutor::named(vec![]);
            let error =
                failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
            assert_eq!(error, expected, "args: {args}");
        }
    }

    #[tokio::test]
    async fn mcp_dict_violation_exhausts_on_third() {
        // Non-dict MCP arguments retry against the run-step budget
        // (`mcp.py:2450`): three consecutive failures exhaust with a
        // count of 2.
        let board = harness(
            vec![tool_reply(vec![("c1", "mcp_add", "5")])],
            vec![mcp_add_wire_tool(), output_wire_tool()],
            vec![],
            vec![mcp_add_schema()],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let error = failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
        assert_eq!(error, "Tool 'mcp_add' exceeded max retries count of 2");
        assert_eq!(board.state.requests.lock().unwrap().len(), 3);
        assert!(executor.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn mcp_twin_error_is_masked() {
        // `mcpprobe.json` `mcp_boom`: the secret never surfaces.
        let board = harness(
            vec![tool_reply(vec![("c1", "mcp_add", r#"{"a": 1}"#)])],
            vec![mcp_add_wire_tool(), output_wire_tool()],
            vec![],
            vec![mcp_add_schema()],
        )
        .await;
        let executor = FakeExecutor::named(vec![(
            "mcp_add",
            Behavior::Fatal("secret-detail".to_owned()),
        )]);
        let error = failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
        assert_eq!(error, "Error calling tool 'mcp_add'");
    }

    #[tokio::test]
    async fn mcp_timeout_surfaces() {
        let mut board = harness(
            vec![tool_reply(vec![("c1", "mcp_add", r#"{"a": 1}"#)])],
            vec![mcp_add_wire_tool(), output_wire_tool()],
            vec![],
            vec![mcp_add_schema()],
        )
        .await;
        board.mcp_timeout_secs = 1;
        let executor = FakeExecutor::named(vec![(
            "mcp_add",
            Behavior::Slow(5, Value::String("late".to_owned())),
        )]);
        match run_loop(&board.config(), &executor).await.unwrap_err() {
            LoopError::Timeout => {}
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn execution_retry_recovers() {
        let board = harness(
            vec![
                tool_reply(vec![("c1", "add", r#"{"a": 1, "b": 2}"#)]),
                tool_reply(vec![("c2", "add", r#"{"a": 1, "b": 2}"#)]),
                text_reply(&done_text("completed", "retried")),
            ],
            vec![add_wire_tool(), output_wire_tool()],
            vec![add_schema()],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![(
            "add",
            Behavior::RetryThenOk(Value::Number(3.into())),
        )]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "retried");
        assert_eq!(output.usage.tool_calls, 1);
        let requests = board.state.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[1]["messages"][2]["content"]
            .as_str()
            .unwrap()
            .starts_with("try again"));
    }

    #[tokio::test]
    async fn truncated_tool_call_reports_incomplete() {
        // `length` + invalid tool args, exhausted: the
        // `IncompleteToolCall` message wins over the retry error.
        let board = harness(
            vec![chat_reply(
                serde_json::json!({
                    "role": "assistant", "content": null,
                    "tool_calls": [{
                        "id": "c1", "type": "function",
                        "function": {"name": "add", "arguments": r#"{"a": 1,"#},
                    }],
                }),
                "length",
            )],
            vec![add_wire_tool(), output_wire_tool()],
            vec![add_schema()],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![("add", Behavior::Add)]);
        let error = failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
        assert!(
            error.contains("while generating a tool call"),
            "got: {error}"
        );
    }

    fn thinking_reply(reasoning: &str, finish: &str) -> MockReply {
        chat_reply(
            serde_json::json!({
                "role": "assistant", "content": "",
                "reasoning_content": reasoning,
            }),
            finish,
        )
    }

    #[tokio::test]
    async fn thinking_only_length_errors() {
        // The length arm fires for thinking-only responses too
        // (`_agent_graph.py:1107-1116`), not just empty ones.
        let board = harness(
            vec![thinking_reply("plan", "length")],
            vec![output_wire_tool()],
            vec![],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let error = failed_message(run_loop(&board.config(), &executor).await.unwrap_err());
        assert!(
            error.starts_with(
                "Model token limit (4096) exceeded before any response was generated."
            ),
            "got: {error}"
        );
        assert_eq!(board.state.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn thinking_only_retries_with_prompt_then_completes() {
        // The thinking-only arm (`_agent_graph.py:1107-1170`): no
        // recoverable text, so the follow-up replays the reasoning
        // and carries the retry prompt — not an empty resubmit.
        let board = harness(
            vec![
                thinking_reply("let me think", "stop"),
                text_reply(&done_text("completed", "thought")),
            ],
            vec![output_wire_tool()],
            vec![],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "thought");
        let requests = board.state.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let second = requests[1]["messages"].as_array().unwrap();
        let assistant = second
            .iter()
            .find(|message| message["role"] == "assistant")
            .expect("assistant replay");
        assert_eq!(assistant["content"], Value::Null);
        assert_eq!(
            assistant["reasoning_content"],
            Value::String("let me think".to_owned())
        );
        assert_eq!(
            second.last().unwrap(),
            &serde_json::json!({
                "role": "user",
                "content": "Validation feedback:\nPlease return text or include your response in a tool call.\n\nFix the errors and try again.",
            })
        );
    }

    #[tokio::test]
    async fn think_tags_in_text_validate() {
        // Tags split out of the text before output validation (the
        // issue's case (a)): the run completes in one request.
        let board = harness(
            vec![text_reply(&format!(
                "<think>plan</think>{}",
                done_text("completed", "tagged")
            ))],
            vec![output_wire_tool()],
            vec![],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "tagged");
        assert_eq!(board.state.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn thinking_beside_tool_call_replays() {
        // Reasoning beside a tool call replays on the follow-up
        // request (the issue's case (c)).
        let board = harness(
            vec![
                chat_reply(
                    serde_json::json!({
                        "role": "assistant", "content": null,
                        "reasoning_content": "plan the add",
                        "tool_calls": [{
                            "id": "c1", "type": "function",
                            "function": {"name": "add", "arguments": r#"{"a": 2, "b": 3}"#},
                        }],
                    }),
                    "tool_calls",
                ),
                text_reply(&done_text("completed", "added")),
            ],
            vec![add_wire_tool(), output_wire_tool()],
            vec![add_schema()],
            vec![],
        )
        .await;
        let executor = FakeExecutor::named(vec![("add", Behavior::Add)]);
        let output = run_loop(&board.config(), &executor).await.expect("runs");
        assert_eq!(output.output.summary(), "added");
        let requests = board.state.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let second = requests[1]["messages"].as_array().unwrap();
        let assistant = second
            .iter()
            .find(|message| message["role"] == "assistant")
            .expect("assistant replay");
        assert_eq!(assistant["content"], Value::Null);
        assert_eq!(
            assistant["reasoning_content"],
            Value::String("plan the add".to_owned())
        );
        assert_eq!(
            assistant["tool_calls"],
            serde_json::json!([{
                "id": "c1", "type": "function",
                "function": {"name": "add", "arguments": r#"{"a": 2, "b": 3}"#},
            }])
        );
    }

    #[test]
    fn recover_text_skips_thinking() {
        // `_recover_text_from_message_history` accumulates `TextPart`
        // only: thinking never recovers as text.
        let history = vec![HistoryMessage::Response {
            parts: vec![ResponsePart::Thinking {
                id: "reasoning_content".to_owned(),
                content: "plan".to_owned(),
            }],
            usage: crate::oss_worker::model_runtime::usage::RequestUsage::zeros(),
            model_name: String::new(),
            finish: Some(Finish::Stop),
            provider_details: None,
            response_id: None,
        }];
        assert_eq!(recover_text(&history), None);
    }
}
