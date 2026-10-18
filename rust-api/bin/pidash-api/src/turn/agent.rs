//! The turn agent loop (PIDASHCONV-863, phase 6b): `assistant.run(...)`.
//!
//! Port of `assistant/tasks.py:337-345`:
//!
//! ```text
//! result = await assistant.run(
//!     ctx.user_text,
//!     model=model,
//!     deps=ctx.deps,
//!     message_history=hist,
//!     toolsets=toolsets,
//!     usage_limits=UsageLimits(request_limit=25, tool_calls_limit=20),
//!     event_stream_handler=streamer.handle,
//! )
//! ```
//!
//! i.e. pydantic-ai 1.107.0's `CallToolsNode` / `HandleResponse` graph
//! (`_agent_graph.py`) for the turn's configuration: `output_type=str`
//! (the `Agent()` default, `runtime/agent.py:26-34`), `retries=2`,
//! `end_strategy='early'`, no output *toolset*, no deferred / native /
//! server tools, no model settings, tools cherry-picked per run
//! (the 17 local tools registered on the shared agent, plus the
//! per-run MCP toolsets).
//!
//! Rules ported here, each with its graph line, in the order one turn
//! walks them:
//!
//! * One request per step: `check_before_request` before the request
//!   (`:960`), the provider POST via [`stream_turn`], then
//!   `_append_response`'s `usage.incr(response.usage)` + `check_tokens`
//!   (`:1023-1026`) and the response appended to history.
//! * Empty / thinking-only responses (`:1107-1171`): a `length`
//!   finish raises the token-limit message naming `last_max_tokens`
//!   (always `None` here — the turn sets no model settings, so
//!   "provider default"); an empty response with `content_filter`
//!   raises `ContentFilterError` with the pretty, ASCII-escaped
//!   dumped response as its body; recoverable text from history is
//!   used as the result; an empty response resubmits an empty request
//!   (no retry part) and spends one output retry; a non-empty
//!   non-actionable response gets the textless retry prompt
//!   (`Please return text or call a tool.` — `alternatives` is
//!   `['return text', 'call a tool']` for this configuration, `/` not
//!   the D-11 `include your response in a tool call`).
//! * Tool calls win over text (`:1246-1250`): a response with any
//!   tool-call part runs the tools and discards its text from the
//!   result (the text still streams to the handler and lands in
//!   history).
//! * The tool-call step (`:1700-1730` `process_tool_calls`, `:1862`
//!   `_call_tools`): the usage gate runs over the projected count
//!   (current + pending) before anything validates; validation runs
//!   sequentially in call order and its failures append retry parts
//!   there; the valid calls then execute and their results append in
//!   call order; `usage.tool_calls` counts successes only.
//! * Retries (`tool_manager.py:177-181,419-450`): the per-name count is
//!   consecutive — the map is rebuilt from the previous step's failed
//!   set only (`for_run_step`), at the cap the run raises
//!   `Tool 'name' exceeded max retries count of 2`; unknown names
//!   budget like function tools and retry with the sorted available
//!   list. All three kinds get `retries=2`.
//! * The output-retry budget (`consume_output_retry`, `:162-175`) is
//!   shared: `Exceeded maximum output retries (2)`, preceded by the
//!   truncated-tool-call check (`check_incomplete_tool_call`, `:146`).
//!
//! The tool side is the [`Toolset`] boundary: the loop owns the
//! pydantic-ai mechanics (retry counting, retry-prompt rendering,
//! usage accounting, the unknown-tool message, the final error
//! classification) and the implementor owns "validate then execute one
//! call", which is exactly the settled PIDASHCONV-1113
//! `tools_local::ToolFailure` shape. Phase 7 (the live seam) binds the
//! 17 local executors and the PIDASHCONV-1112 MCP sessions to it; the
//! tests below drive the loop with scripted toolsets over the
//! `fixtures/assistant/turn-runtime/` captures replayed through a stub
//! provider.
//!
//! Divergences, deliberate and listed for the PR:
//!
//! * Tool-call part starts are not observable through
//!   [`stream_turn`]'s `on_event` (it forwards text events only), so
//!   the `NonTextBoundary` stream item is derived from the part
//!   indices on the next `TextStart` and, for a response that ends on a
//!   tool call, emitted once the response is assembled. Text that
//!   follows a tool-call block inside one response still opens a new
//!   streaming row (the index gap is detected); a response whose text
//!   and tool calls interleave with *no* later text is streamed
//!   identically.
//! * Validation and execution are one boundary call here (1113's
//!   executor validates internally), where pydantic-ai validates the
//!   whole batch before executing any of it. The observable difference
//!   needs several tool calls in one response about to exhaust their
//!   retry budget at once.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use pidash_jobs::assistant::run_turn::{AgentError, StreamItem};
use pidash_jobs::dispatch_execute::UsageLimits;
use pidash_jobs::oss_worker::model_runtime::provider::{
    generate_tool_call_id, Finish, Lane, ProviderError, ToolArgs, WireTool,
};
use pidash_jobs::oss_worker::model_runtime::usage::{
    check_before_request, check_before_tool_call, check_tokens, ProviderTable, RunUsage,
};
use pidash_jobs::oss_worker::model_runtime::validation::unknown_tool_content;

use super::history::{
    now_stamp, parse_retry_details, RetryErrorDetail, RunIds, TurnFinish, TurnMessage, TurnOutcome,
    TurnRequestPart, TurnResponsePart, TurnRetryContent,
};
use super::streaming::{
    anthropic_request_body, encode_body, openai_request_body, stream_endpoint, stream_turn,
    StreamEvent, StreamPart, StreamRequest,
};

/// `UsageLimits(request_limit=25, tool_calls_limit=20)` (`tasks.py:342`).
///
/// Built from the constants in `pidash_jobs::assistant::run_turn` so the
/// drive and the live run cannot drift apart.
pub fn turn_usage_limits() -> UsageLimits {
    UsageLimits {
        request_limit: Some(i64::from(pidash_jobs::assistant::run_turn::REQUEST_LIMIT)),
        tool_calls_limit: Some(i64::from(
            pidash_jobs::assistant::run_turn::TOOL_CALLS_LIMIT,
        )),
        input_tokens_limit: None,
        output_tokens_limit: None,
        total_tokens_limit: None,
    }
}

/// `Agent(retries=2)` (`runtime/agent.py:30`): the same budget for tool
/// retries (`Agent._max_tool_retries`) and output retries
/// (`Agent._max_output_retries`).
pub const MAX_RETRIES: u64 = 2;

/// Transport timeout for one provider request: the SDK default the
/// captures pin (`x-stainless-read-timeout: 600`).
pub const DEFAULT_TIMEOUT_SECS: u64 = 600;

/// The textless retry prompt for this configuration
/// (`_agent_graph.py:1246-1258`): `output_type=str` contributes
/// `return text`, the non-empty toolset contributes `call a tool`.
pub const TEXTLESS_RETRY_TEXT: &str = "Please return text or call a tool.";

// ---------------------------------------------------------------------------
// The tool boundary
// ---------------------------------------------------------------------------

/// How the loop classifies one model-called tool name
/// (`ToolManager._resolve_tool` + `ToolDefinition.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    /// A function tool: a local executor (PIDASHCONV-1113) or an MCP
    /// tool reached through a per-run toolset.
    Function,
    /// An MCP tool (PIDASHCONV-1112). The loop treats it like a
    /// function tool — pydantic-ai does too — but keeps the variant so
    /// the seam can report which leg ran.
    Mcp,
    /// No definition: the retry prompt names the available tools.
    Unknown,
}

/// How one tool call ended, mapped 1:1 onto PIDASHCONV-1113's
/// `tools_local::ToolFailure` plus success.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolCallOutcome {
    /// The tool returned this value (`ToolReturnPart.content`).
    Ok(Value),
    /// A `ModelRetry` message → a text retry prompt.
    Retry(String),
    /// Pydantic arg-validation details (`list[ErrorDetails]`) → an
    /// error-list retry prompt.
    InvalidArgs(Value),
    /// Anything escaping the tool: fails the turn, never retries.
    Internal(String),
}

/// The tool side of one turn: definitions for the request body, the
/// kind of each name, and "validate then execute one call".
///
/// Validation lives with the implementor because 1113's executors own
/// their own arg validation; their failure shapes cross back as
/// [`ToolCallOutcome::InvalidArgs`] / [`ToolCallOutcome::Retry`].
pub trait Toolset: Send + Sync {
    /// Wire tools in registration order: the local functions
    /// (`@assistant.tool` import order, sorted by name) then the
    /// per-run MCP toolsets.
    fn wire_tools(&self) -> Vec<WireTool>;
    /// Every model-facing tool name, sorted (`ToolManager.tools` keys,
    /// the unknown-tool message order).
    fn names_sorted(&self) -> Vec<String>;
    /// The kind of one model-facing name.
    fn kind(&self, name: &str) -> ToolKind;
    /// Validate one call's arguments and execute it. `args` is
    /// `ToolCallPart.args_as_dict()` — never the `INVALID_JSON` wrap
    /// (`args_as_dict(raise_if_invalid=True)` semantics).
    fn call(
        &self,
        name: &str,
        args: Map<String, Value>,
        call_id: &str,
    ) -> impl std::future::Future<Output = ToolCallOutcome> + Send;
}

// ---------------------------------------------------------------------------
// Configuration, result, errors
// ---------------------------------------------------------------------------

/// One turn's agent run: everything `assistant.run(...)` receives.
pub struct TurnAgentConfig<'a> {
    /// The resolved model lane (headers, auth, endpoint, model name).
    pub lane: &'a Lane,
    /// HTTP client for the provider requests.
    pub client: &'a reqwest::Client,
    /// The usage-extractor table (`ProviderTable`).
    pub extract: &'a ProviderTable,
    /// Instruction parts in order (static, then dynamic).
    pub instructions: &'a [String],
    /// Replayed history, oldest first (prior completed turns).
    pub history: &'a [TurnMessage],
    /// This turn's user prompt text.
    pub prompt: &'a str,
    /// The run's usage limits.
    pub limits: &'a UsageLimits,
    /// Per-request transport timeout, seconds.
    pub timeout_secs: u64,
    /// Streaming endpoint override; `None` uses the lane's real
    /// endpoint ([`stream_endpoint`]).
    pub endpoint_url: Option<String>,
}

/// What one successful run produced: `result.new_messages()` and
/// `result.usage()`, plus the text output (the str output for the
/// turn).
#[derive(Debug)]
pub struct TurnAgentOutcome {
    pub messages: Vec<TurnMessage>,
    pub usage: RunUsage,
    pub output: String,
}

/// How the run ended before producing output; both arms classify the
/// same way Python's two `except` arms do (`tasks.py:351-361`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnLoopError {
    /// `UsageLimitExceeded` → `iteration_limit` (verbatim message).
    UsageLimit(String),
    /// Any other failure, classified from its message (the typed-first
    /// walk has no counterpart here: pydantic-ai raises untyped
    /// errors, and an ee provider's typed error crosses the seam).
    Provider {
        message: String,
        status_code: Option<u16>,
    },
}

impl From<TurnLoopError> for AgentError {
    fn from(error: TurnLoopError) -> AgentError {
        match error {
            TurnLoopError::UsageLimit(message) => AgentError::UsageLimit(message),
            TurnLoopError::Provider {
                message,
                status_code,
            } => AgentError::Provider {
                message,
                status_code,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// The loop
// ---------------------------------------------------------------------------

/// Mutable per-run state (`GraphAgentState` subset):
/// `message_history` is the request-building history (replayed plus
/// new), `new_messages` is `result.new_messages()`.
struct RunState {
    ids: RunIds,
    history: Vec<TurnMessage>,
    new_messages: Vec<TurnMessage>,
    usage: RunUsage,
    retries: HashMap<String, u64>,
    output_retries_used: u64,
}

impl RunState {
    fn new(ids: RunIds, history: &[TurnMessage]) -> Self {
        RunState {
            ids,
            history: history.to_vec(),
            new_messages: Vec::new(),
            usage: RunUsage::default(),
            retries: HashMap::new(),
            output_retries_used: 0,
        }
    }

    /// Append one message to both the request history and the new-message
    /// list (`new_messages()` is the run-stamped slice of the history).
    fn push(&mut self, message: TurnMessage) {
        self.history.push(message.clone());
        self.new_messages.push(message);
    }

    /// `consume_output_retry` (`_agent_graph.py:162-175`): spend one
    /// unit of output budget, raising past the cap after the
    /// truncated-tool-call check.
    fn consume_output_retry(&mut self) -> Result<(), TurnLoopError> {
        self.output_retries_used += 1;
        if self.output_retries_used > MAX_RETRIES {
            if let Some(message) = self.incomplete_tool_call() {
                return Err(TurnLoopError::Provider {
                    message,
                    status_code: None,
                });
            }
            return Err(TurnLoopError::Provider {
                message: format!("Exceeded maximum output retries ({MAX_RETRIES})"),
                status_code: None,
            });
        }
        Ok(())
    }

    /// `check_incomplete_tool_call` (`_agent_graph.py:146-160`): the
    /// truncated-call message when the last response ended `length` on
    /// a tool call whose arguments do not parse.
    fn incomplete_tool_call(&self) -> Option<String> {
        let TurnMessage::Response { parts, finish, .. } = self.history.last()? else {
            return None;
        };
        if *finish != Some(TurnFinish::Length) {
            return None;
        }
        let TurnResponsePart::ToolCall { args, .. } = parts.last()? else {
            return None;
        };
        if args_as_dict_checked(args).is_ok() {
            return None;
        }
        Some(
            "Model token limit (provider default) exceeded while generating a tool call, resulting in incomplete arguments. Increase the `max_tokens` model setting, or simplify the prompt to result in a shorter response that will fit within the limit."
                .to_owned(),
        )
    }

    /// `_check_max_retries` (`tool_manager.py:177-181`) for one name.
    fn check_max_retries(&self, name: &str) -> Result<(), TurnLoopError> {
        if self.retries.get(name).copied().unwrap_or(0) == MAX_RETRIES {
            if let Some(message) = self.incomplete_tool_call() {
                return Err(TurnLoopError::Provider {
                    message,
                    status_code: None,
                });
            }
            return Err(TurnLoopError::Provider {
                message: format!("Tool '{name}' exceeded max retries count of {MAX_RETRIES}"),
                status_code: None,
            });
        }
        Ok(())
    }
}

/// Run one turn's agent loop (`assistant.run`).
///
/// Stream items go to `emit` as the provider streams them, in the
/// order `_Streamer.handle` sees them: text starts and deltas, a
/// non-text boundary when a non-text part begins, and one `End` after
/// the run completes (the handler's trailing `_finalize`). Nothing is
/// emitted on a failing run — the drive's `fail_open` path owns the
/// open row, exactly like `except` in `_run_turn`.
pub async fn run_agent<T, F>(
    config: &TurnAgentConfig<'_>,
    toolset: &T,
    mut emit: F,
) -> Result<TurnAgentOutcome, TurnLoopError>
where
    T: Toolset,
    F: FnMut(StreamItem) + Send,
{
    let mut state = RunState::new(RunIds::fresh(), config.history);
    let first = TurnMessage::new_request(
        vec![TurnRequestPart::User {
            text: config.prompt.to_owned(),
            timestamp: Some(now_stamp()),
        }],
        joined_instructions(config.instructions),
        &state.ids,
        &now_stamp(),
    );
    state.push(first);

    let wire_tools = toolset.wire_tools();
    let names_sorted = toolset.names_sorted();

    loop {
        if let Err(breach) = check_before_request(config.limits, &state.usage) {
            return Err(TurnLoopError::UsageLimit(breach.0));
        }
        let body = request_body(config, &state.history, &wire_tools);
        let request = StreamRequest {
            lane: config.lane,
            endpoint_url: config
                .endpoint_url
                .clone()
                .unwrap_or_else(|| stream_endpoint(config.lane)),
            body: encode_body(&body),
            timeout_secs: config.timeout_secs,
            extract: config.extract,
        };
        let mut last_text_index: Option<usize> = None;
        let streamed = {
            let emit = &mut emit;
            let last = &mut last_text_index;
            stream_turn(config.client, &request, |event| match event {
                StreamEvent::TextStart { index, initial } => {
                    // A gap means a non-text part started in between
                    // (`PartStartEvent` for a tool call / thinking
                    // part): the streamer finalizes the open row
                    // there.
                    if let Some(previous) = *last {
                        if index > previous + 1 {
                            emit(StreamItem::NonTextBoundary);
                        }
                    }
                    *last = Some(index);
                    emit(StreamItem::TextStart { initial });
                }
                StreamEvent::TextDelta { delta, .. } => {
                    emit(StreamItem::TextDelta { chunk: delta });
                }
            })
            .await
        }
        .map_err(|error| provider_error(config.lane, &error))?;

        state.usage.incr(&streamed.usage);
        state.usage.requests += 1;
        if let Err(breach) = check_tokens(config.limits, &state.usage) {
            return Err(TurnLoopError::UsageLimit(breach.0));
        }

        // Snapshot the response-level fields before the parts move into
        // the stored message.
        let response_usage = streamed.usage.clone();
        let response_model_name = streamed.model_name.clone();
        let response_details = streamed.provider_details.clone();
        let response_id = streamed.response_id.clone();
        let response_finish = streamed.finish;

        let mut parts: Vec<TurnResponsePart> = Vec::new();
        let mut texts: Vec<String> = Vec::new();
        let mut calls: Vec<ModelToolCall> = Vec::new();
        for part in streamed.parts {
            match part {
                StreamPart::Text(text) => {
                    texts.push(text.clone());
                    parts.push(TurnResponsePart::Text {
                        text,
                        id: None,
                        provider_name: None,
                        provider_details: None,
                    });
                }
                StreamPart::ToolCall(call) => {
                    calls.push(ModelToolCall {
                        name: call.name.clone(),
                        args: call.args.clone(),
                        call_id: call.call_id.clone(),
                    });
                    parts.push(TurnResponsePart::ToolCall {
                        name: call.name,
                        args: call.args,
                        call_id: call.call_id,
                        id: None,
                        provider_name: None,
                        provider_details: None,
                    });
                }
            }
        }
        let response = TurnMessage::new_response(
            parts,
            response_usage,
            Some(response_model_name),
            &now_stamp(),
            Some(config.lane.provider_name().to_owned()),
            Some(config.lane.provider_url()),
            response_details,
            response_id,
            response_finish.map(finish_reason),
            &state.ids,
        );

        if response_is_empty(&response) {
            state.push(response);
            // (`_agent_graph.py:1107-1191`.) `last_max_tokens` is always
            // `None` for the turn: no model settings are set.
            match response_finish {
                Some(Finish::Length) => {
                    return Err(TurnLoopError::Provider {
                        message: LENGTH_MESSAGE.to_owned(),
                        status_code: None,
                    });
                }
                Some(Finish::ContentFilter) => {
                    return Err(TurnLoopError::Provider {
                        message: content_filter_message(&state),
                        status_code: None,
                    });
                }
                _ => {}
            }
            if let Some(text) = recover_text(&state.new_messages) {
                return Ok(finish(state, text, &mut emit));
            }
            // An empty response resubmits an empty request and spends
            // one output retry.
            state.consume_output_retry()?;
            state.retries.clear();
            let request = TurnMessage::new_request(
                Vec::new(),
                joined_instructions(config.instructions),
                &state.ids,
                &now_stamp(),
            );
            state.push(request);
            continue;
        }

        if !calls.is_empty() {
            // The tool-call part start finalized any open row.
            emit(StreamItem::NonTextBoundary);
            state.push(response);
            let step = process_tool_calls(config, toolset, &state, &calls, &names_sorted).await?;
            state.usage.tool_calls += step.successes;
            state.retries = for_run_step(&state.retries, &step.failed);
            let request = TurnMessage::new_request(
                step.parts,
                joined_instructions(config.instructions),
                &state.ids,
                &now_stamp(),
            );
            state.push(request);
            continue;
        }

        // Text-only response. Text alongside tool calls is discarded
        // above; here the text is the result.
        let text = texts.concat();
        state.push(response);
        let recovered = if text.is_empty() {
            recover_text(&state.new_messages)
        } else {
            Some(text)
        };
        let Some(text) = recovered else {
            // Non-actionable parts only (thinking-only responses):
            // the textless retry prompt.
            state.consume_output_retry()?;
            state.retries.clear();
            let request = TurnMessage::new_request(
                vec![TurnRequestPart::Retry {
                    tool_name: None,
                    content: TurnRetryContent::Text(TEXTLESS_RETRY_TEXT.to_owned()),
                    call_id: generate_tool_call_id(),
                    timestamp: Some(now_stamp()),
                }],
                joined_instructions(config.instructions),
                &state.ids,
                &now_stamp(),
            );
            state.push(request);
            continue;
        };
        return Ok(finish(state, text, &mut emit));
    }
}

/// The length-exhaustion message (`_agent_graph.py:1123-1128`): the
/// turn never sets `max_tokens`, so the placeholder is
/// "provider default".
const LENGTH_MESSAGE: &str = "Model token limit (provider default) exceeded before any response was generated. Increase the `max_tokens` model setting, or simplify the prompt to result in a shorter response that will fit within the limit.";

/// Close the run: emit the handler's trailing finalize and hand back
/// the outcome.
fn finish<F: FnMut(StreamItem)>(state: RunState, output: String, emit: &mut F) -> TurnAgentOutcome {
    emit(StreamItem::End);
    TurnAgentOutcome {
        messages: state.new_messages,
        usage: state.usage,
        output,
    }
}

/// One model-called tool, in response order.
struct ModelToolCall {
    name: String,
    args: Option<ToolArgs>,
    call_id: String,
}

/// What one tool-call step produced.
struct StepOutcome {
    parts: Vec<TurnRequestPart>,
    successes: i64,
    failed: HashSet<String>,
}

/// `process_tool_calls` / `_call_tools` for this configuration
/// (`_agent_graph.py:1700-2050`).
async fn process_tool_calls<T: Toolset>(
    config: &TurnAgentConfig<'_>,
    toolset: &T,
    state: &RunState,
    calls: &[ModelToolCall],
    names_sorted: &[String],
) -> Result<StepOutcome, TurnLoopError> {
    // The usage gate runs over the projected count before anything
    // validates (`:1703-1706`).
    let projected = state.usage.tool_calls + calls.len() as i64;
    if let Err(breach) = check_before_tool_call(config.limits, projected) {
        return Err(TurnLoopError::UsageLimit(breach.0));
    }

    let mut parts: Vec<TurnRequestPart> = Vec::new();
    let mut successes: i64 = 0;
    let mut failed: HashSet<String> = HashSet::new();

    // Validation runs in call order; the valid calls then execute and
    // append in call order (`:1862-1980`). Both halves are one boundary
    // call per tool here (see the module docs).
    for call in calls {
        match toolset.kind(&call.name) {
            ToolKind::Unknown => {
                let available: Vec<&str> = names_sorted.iter().map(String::as_str).collect();
                let message = unknown_tool_content(&call.name, &available);
                state.check_max_retries(&call.name)?;
                failed.insert(call.name.clone());
                parts.push(retry_text_part(&call.name, &message, &call.call_id));
            }
            ToolKind::Function | ToolKind::Mcp => {
                let args = args_as_dict_checked(&call.args).map_err(|message| {
                    TurnLoopError::Provider {
                        message,
                        status_code: None,
                    }
                })?;
                match toolset.call(&call.name, args, &call.call_id).await {
                    ToolCallOutcome::Ok(value) => {
                        successes += 1;
                        parts.push(tool_return_part(&call.name, value, &call.call_id));
                    }
                    ToolCallOutcome::Retry(message) => {
                        state.check_max_retries(&call.name)?;
                        failed.insert(call.name.clone());
                        parts.push(retry_text_part(&call.name, &message, &call.call_id));
                    }
                    ToolCallOutcome::InvalidArgs(details) => {
                        state.check_max_retries(&call.name)?;
                        failed.insert(call.name.clone());
                        parts.push(retry_details_part(&call.name, &details, &call.call_id)?);
                    }
                    ToolCallOutcome::Internal(message) => {
                        return Err(TurnLoopError::Provider {
                            message,
                            status_code: None,
                        });
                    }
                }
            }
        }
    }
    Ok(StepOutcome {
        parts,
        successes,
        failed,
    })
}

/// `for_run_step` (`tool_manager.py:120-140`): the per-name count is
/// consecutive — each step rebuilds the map from the previous step's
/// failed set only.
fn for_run_step(retries: &HashMap<String, u64>, failed: &HashSet<String>) -> HashMap<String, u64> {
    failed
        .iter()
        .map(|name| (name.clone(), retries.get(name).copied().unwrap_or(0) + 1))
        .collect()
}

/// The OpenAI-lane user message (`_map_messages`).
fn request_body(
    config: &TurnAgentConfig<'_>,
    history: &[TurnMessage],
    tools: &[WireTool],
) -> Value {
    match config.lane {
        Lane::OpenAICompatible { .. } => openai_request_body(
            config.lane.model_name(),
            config.instructions,
            history,
            tools,
        ),
        Lane::Anthropic { .. } => anthropic_request_body(
            config.lane.model_name(),
            config.instructions,
            history,
            tools,
        ),
    }
}

/// `FinishReason` mapping (`messages.py:117-123`).
fn finish_reason(finish: Finish) -> TurnFinish {
    match finish {
        Finish::Stop => TurnFinish::Stop,
        Finish::Length => TurnFinish::Length,
        Finish::ToolCall => TurnFinish::ToolCall,
        Finish::ContentFilter => TurnFinish::ContentFilter,
    }
}

fn response_is_empty(message: &TurnMessage) -> bool {
    match message {
        TurnMessage::Response { parts, .. } => parts.is_empty(),
        TurnMessage::Request { .. } => false,
    }
}

/// `_recover_text_from_message_history` (`_agent_graph.py:1301-1320`):
/// the newest response whose concatenated text is non-empty.
fn recover_text(messages: &[TurnMessage]) -> Option<String> {
    for message in messages.iter().rev() {
        if let TurnMessage::Response { parts, .. } = message {
            let text: String = parts
                .iter()
                .filter_map(|part| match part {
                    TurnResponsePart::Text { text, .. } => Some(text.as_str()),
                    TurnResponsePart::ToolCall { .. } => None,
                })
                .collect();
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

/// `ToolCallPart.args_as_dict(raise_if_invalid=True)`
/// (`messages.py:1838-1864`): absent or empty args read `{}`; a raw
/// arguments string must parse as a JSON object.
fn args_as_dict_checked(args: &Option<ToolArgs>) -> Result<Map<String, Value>, String> {
    let raw = match args {
        None => return Ok(Map::new()),
        Some(ToolArgs::Dict(map)) => return Ok(map.clone()),
        Some(ToolArgs::Raw(raw)) => raw,
    };
    if raw.is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(other) => Err(format!(
            "args should be a dict (got {other}) for the tool call"
        )),
        Err(error) => Err(format!("invalid JSON tool arguments: {error}")),
    }
}

fn tool_return_part(name: &str, content: Value, call_id: &str) -> TurnRequestPart {
    TurnRequestPart::ToolReturn {
        name: name.to_owned(),
        content,
        call_id: call_id.to_owned(),
        timestamp: Some(now_stamp()),
        metadata: Value::Null,
        outcome: TurnOutcome::Success,
    }
}

fn retry_text_part(name: &str, message: &str, call_id: &str) -> TurnRequestPart {
    TurnRequestPart::Retry {
        tool_name: Some(name.to_owned()),
        content: TurnRetryContent::Text(message.to_owned()),
        call_id: call_id.to_owned(),
        timestamp: Some(now_stamp()),
    }
}

fn retry_details_part(
    name: &str,
    details: &Value,
    call_id: &str,
) -> Result<TurnRequestPart, TurnLoopError> {
    let list = details.as_array().ok_or_else(|| TurnLoopError::Provider {
        message: format!("tool '{name}' returned non-array validation details"),
        status_code: None,
    })?;
    let parsed: Vec<RetryErrorDetail> =
        parse_retry_details(list).map_err(|error| TurnLoopError::Provider {
            message: format!("tool '{name}' returned unparsable validation details: {error}"),
            status_code: None,
        })?;
    Ok(TurnRequestPart::Retry {
        tool_name: Some(name.to_owned()),
        content: TurnRetryContent::Errors(parsed),
        call_id: call_id.to_owned(),
        timestamp: Some(now_stamp()),
    })
}

/// `ModelHTTPError` / `APITimeoutError` / `APIConnectionError` from the
/// streaming transport, classified the same way the drive's text
/// fallback does.
fn provider_error(lane: &Lane, error: &ProviderError) -> TurnLoopError {
    let status_code = match error {
        ProviderError::Http { status, .. } => Some(*status),
        ProviderError::Timeout | ProviderError::Connection | ProviderError::InvalidResponse(_) => {
            None
        }
    };
    TurnLoopError::Provider {
        message: error.message(lane),
        status_code,
    }
}

/// `InstructionPart.join` (`messages.py:1537-1539`): double-newline
/// joined, stripped, `None` when empty.
fn joined_instructions(parts: &[String]) -> Option<String> {
    let joined = parts.join("\n\n");
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// `ContentFilterError`'s message plus body (`_agent_graph.py:1134-1144`,
/// `exceptions.py:203-231`): the compact dump of the response,
/// re-parsed and re-dumped with `json.dumps(..., indent=2)` (ASCII
/// escaped).
fn content_filter_message(state: &RunState) -> String {
    let details = match state.history.last() {
        Some(TurnMessage::Response {
            provider_details, ..
        }) => provider_details.clone().unwrap_or_default(),
        _ => Map::new(),
    };
    let present = |key: &str| {
        details
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let message = if let Some(reason) = present("finish_reason") {
        format!("Content filter triggered. Finish reason: '{reason}'")
    } else if let Some(reason) = present("block_reason") {
        format!("Content filter triggered. Block reason: '{reason}'")
    } else if let Some(refusal) = present("refusal") {
        format!(
            "Content filter triggered. Refusal: {}",
            pidash_jobs::oss_worker::model_runtime::provider::py_repr(&Value::String(refusal))
        )
    } else {
        "Content filter triggered.".to_owned()
    };
    let body = match state.history.last() {
        Some(message @ TurnMessage::Response { .. }) => {
            let compact =
                serde_json::to_string(&Value::Array(vec![super::history::dump_message(message)]))
                    .unwrap_or_default();
            let reparsed: Value = serde_json::from_str(&compact).unwrap_or(Value::Null);
            let pretty = serde_json::to_string_pretty(&reparsed).unwrap_or_default();
            ensure_ascii(&pretty)
        }
        _ => String::new(),
    };
    if body.is_empty() {
        message
    } else {
        format!("{message}, body:\n{body}")
    }
}

/// CPython `json.dumps(..., ensure_ascii=True)` escaping for the
/// content-filter body. Reimplemented here because the D-11 copy is
/// `pub(crate)` in `pidash_jobs` (foundation crates are read-only):
/// non-ASCII becomes `\uXXXX` (surrogate pairs above the BMP), and the
/// control characters use CPython's short escapes.
fn ensure_ascii(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) < 0x7f => out.push(c),
            c => {
                let code = c as u32;
                if code > 0xffff {
                    let adjusted = code - 0x10000;
                    let high = 0xd800 + (adjusted >> 10);
                    let low = 0xdc00 + (adjusted & 0x3ff);
                    out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::history::{dump_blob, parse_blob, TurnFinish};
    use super::super::replay::{
        instructions_from_body, is_anthropic, load_fixture, normalize_volatile, prompt_from_body,
        reply_for, request_body_of, request_bytes_of, stub_server, wire_tools_from_body,
    };
    use super::*;
    use pidash_jobs::assistant::run_turn::{classify_error, CODE_ITERATION_LIMIT};
    use pidash_jobs::oss_worker::model_runtime::schemas::Golden;
    use std::collections::BTreeMap;

    /// The D-11 usage-extractor table (embedded golden).
    fn provider_table() -> ProviderTable {
        let golden = Golden::load().expect("golden parses");
        ProviderTable::load(&golden).expect("provider table loads")
    }

    /// A toolset replaying one capture's tools: wire tools from the
    /// captured request, call outcomes keyed by the captured
    /// tool-call ids (returns from `tool-return` parts, arg failures
    /// from `retry-prompt` parts), one default outcome otherwise.
    struct FakeToolset {
        wire: Vec<WireTool>,
        names: Vec<String>,
        outcomes: BTreeMap<String, ToolCallOutcome>,
        default: ToolCallOutcome,
    }

    impl FakeToolset {
        fn from_capture(anthropic: bool, body: &Value, fixture: &Value) -> Self {
            let wire = wire_tools_from_body(anthropic, body);
            let mut names: Vec<String> = wire.iter().map(|tool| tool.name.clone()).collect();
            names.sort();
            let mut outcomes = BTreeMap::new();
            for messages in capture_messages(fixture) {
                let Ok(parsed) = parse_blob(&messages) else {
                    continue;
                };
                for message in parsed {
                    let TurnMessage::Request { parts, .. } = message else {
                        continue;
                    };
                    for part in parts {
                        match part {
                            TurnRequestPart::ToolReturn {
                                content, call_id, ..
                            } => {
                                outcomes.insert(call_id, ToolCallOutcome::Ok(content));
                            }
                            TurnRequestPart::Retry {
                                content,
                                call_id,
                                tool_name,
                                ..
                            } => {
                                if tool_name.is_some() {
                                    let outcome = match content {
                                        TurnRetryContent::Errors(details) => {
                                            ToolCallOutcome::InvalidArgs(
                                                serde_json::to_value(
                                                    details
                                                        .iter()
                                                        .map(detail_to_value)
                                                        .collect::<Vec<_>>(),
                                                )
                                                .unwrap(),
                                            )
                                        }
                                        TurnRetryContent::Text(text) => {
                                            ToolCallOutcome::Retry(text)
                                        }
                                    };
                                    outcomes.insert(call_id, outcome);
                                }
                            }
                            TurnRequestPart::User { .. } => {}
                        }
                    }
                }
            }
            // The stored `model_messages` blobs came back through Postgres
            // `jsonb`, which normalizes object key order (by length, then
            // bytes); the provider wire keeps the tool's own order. Override
            // every Ok content with the wire's so request bytes replay
            // exactly (the blob comparison stays order-insensitive).
            outcomes.extend(wire_tool_results(anthropic, fixture));
            FakeToolset {
                wire,
                names,
                outcomes,
                default: ToolCallOutcome::Ok(Value::String(String::new())),
            }
        }
    }

    impl Toolset for FakeToolset {
        fn wire_tools(&self) -> Vec<WireTool> {
            self.wire.clone()
        }

        fn names_sorted(&self) -> Vec<String> {
            self.names.clone()
        }

        fn kind(&self, name: &str) -> ToolKind {
            if name.starts_with("mcp_") {
                ToolKind::Mcp
            } else {
                ToolKind::Function
            }
        }

        async fn call(
            &self,
            _name: &str,
            _args: Map<String, Value>,
            call_id: &str,
        ) -> ToolCallOutcome {
            self.outcomes
                .get(call_id)
                .cloned()
                .unwrap_or_else(|| self.default.clone())
        }
    }

    /// Dump one retry detail back into the `list[ErrorDetails]` value a
    /// `Toolset` returns.
    fn detail_to_value(detail: &RetryErrorDetail) -> Value {
        Value::Object(Map::from_iter([
            ("type".to_owned(), Value::String(detail.error_type.clone())),
            (
                "loc".to_owned(),
                Value::Array(
                    detail
                        .loc
                        .iter()
                        .map(|segment| match segment {
                            super::super::history::RetryLocSeg::Field(name) => {
                                Value::String(name.clone())
                            }
                            super::super::history::RetryLocSeg::Index(index) => {
                                Value::Number((*index).into())
                            }
                        })
                        .collect(),
                ),
            ),
            ("msg".to_owned(), Value::String(detail.msg.clone())),
            ("input".to_owned(), detail.input.clone()),
        ]))
    }

    /// Every `model_messages` blob in a capture (single-turn captures
    /// carry one; `replay_3turn` carries one per turn).
    fn capture_messages(fixture: &Value) -> Vec<Value> {
        let mut out = Vec::new();
        if let Some(Value::Array(messages)) = fixture.get("model_messages") {
            out.push(Value::Array(messages.clone()));
        }
        if let Some(Value::Array(turns)) = fixture.get("turns") {
            for turn in turns {
                if let Some(messages) = turn.get("model_messages") {
                    out.push(messages.clone());
                }
            }
        }
        out
    }

    /// The `Ok` tool outcomes as the provider wire carries them: the
    /// captured request bodies keep the tool's own key order, unlike the
    /// jsonb-normalized stored blobs.
    fn wire_tool_results(anthropic: bool, fixture: &Value) -> Vec<(String, ToolCallOutcome)> {
        let mut out = Vec::new();
        let Some(exchanges) = fixture.get("provider_exchanges").and_then(Value::as_array) else {
            return out;
        };
        for exchange in exchanges {
            let Some(messages) = exchange
                .get("request")
                .and_then(|request| request.get("body"))
                .and_then(|body| body.get("messages"))
                .and_then(Value::as_array)
            else {
                continue;
            };
            for message in messages {
                if anthropic {
                    let Some(blocks) = message.get("content").and_then(Value::as_array) else {
                        continue;
                    };
                    for block in blocks {
                        if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                            continue;
                        }
                        let Some(call_id) = block.get("tool_use_id").and_then(Value::as_str) else {
                            continue;
                        };
                        let Some(text) = block.get("content").and_then(Value::as_str) else {
                            continue;
                        };
                        if let Ok(value) = serde_json::from_str::<Value>(text) {
                            if value.is_array() || value.is_object() {
                                out.push((call_id.to_owned(), ToolCallOutcome::Ok(value)));
                            }
                        }
                    }
                } else if message.get("role").and_then(Value::as_str) == Some("tool") {
                    let Some(call_id) = message.get("tool_call_id").and_then(Value::as_str) else {
                        continue;
                    };
                    let Some(text) = message.get("content").and_then(Value::as_str) else {
                        continue;
                    };
                    if let Ok(value) = serde_json::from_str::<Value>(text) {
                        if value.is_array() || value.is_object() {
                            out.push((call_id.to_owned(), ToolCallOutcome::Ok(value)));
                        }
                    }
                }
            }
        }
        out
    }

    struct RunOutcome {
        result: Result<TurnAgentOutcome, TurnLoopError>,
        events: Vec<StreamItem>,
        seen: Vec<super::super::replay::StubRequest>,
    }

    /// Run one captured exchange through the loop against a stub
    /// provider replaying that exchange's reply.
    async fn run_capture(
        fixture: &Value,
        index: usize,
        history: &[Value],
        replies: Vec<super::super::replay::StubReply>,
    ) -> RunOutcome {
        let anthropic = is_anthropic(fixture);
        let exchange = &fixture["provider_exchanges"][index];
        let body = request_body_of(exchange);
        let instructions = instructions_from_body(anthropic, &body);
        let prompt = prompt_from_body(&body);
        let wire = wire_tools_from_body(anthropic, &body);
        assert!(!wire.is_empty(), "capture carries wire tools");
        let toolset = FakeToolset::from_capture(anthropic, &body, fixture);
        let table = provider_table();
        let (base, state) = stub_server(replies).await;
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .unwrap()
            .to_owned();
        let lane = if anthropic {
            Lane::Anthropic {
                model,
                api_key: "test-key-123".to_owned(),
            }
        } else {
            Lane::OpenAICompatible {
                model,
                base_url: format!("{base}/v1"),
                api_key: "test-key-123".to_owned(),
            }
        };
        let mut replayed: Vec<TurnMessage> = Vec::new();
        for blob in history {
            replayed.extend(parse_blob(blob).expect("history blob parses"));
        }
        let limits = turn_usage_limits();
        let config = TurnAgentConfig {
            lane: &lane,
            client: &reqwest::Client::new(),
            extract: &table,
            instructions: &instructions,
            history: &replayed,
            prompt: &prompt,
            limits: &limits,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            endpoint_url: Some(match &lane {
                Lane::OpenAICompatible { base_url, .. } => {
                    format!("{}/chat/completions", base_url.trim_end_matches('/'))
                }
                Lane::Anthropic { .. } => format!("{base}/v1/messages?beta=true"),
            }),
        };
        let mut events: Vec<StreamItem> = Vec::new();
        let result = run_agent(&config, &toolset, |item| events.push(item)).await;
        RunOutcome {
            result,
            events,
            seen: state.seen(),
        }
    }

    /// Every exchange of a single-turn capture replays byte for byte,
    /// and the produced messages/usage equal the stored ones.
    async fn one_turn_capture(name: &str) {
        let fixture = load_fixture(name);
        let exchanges = fixture["provider_exchanges"].as_array().unwrap();
        let replies: Vec<_> = exchanges.iter().map(reply_for).collect();
        let outcome = run_capture(&fixture, 0, &[], replies).await;
        assert_eq!(
            outcome.seen.len(),
            exchanges.len(),
            "{name}: one request per exchange"
        );
        for (index, seen) in outcome.seen.iter().enumerate() {
            assert_eq!(
                seen.body,
                request_bytes_of(&fixture["provider_exchanges"][index]),
                "{name}#{index}: request bytes"
            );
        }
        let result = outcome
            .result
            .unwrap_or_else(|error| panic!("{name}: run must succeed, got {error:?}"));
        assert_eq!(
            normalize_volatile(&dump_blob(&result.messages)),
            normalize_volatile(&fixture["model_messages"]),
            "{name}: new messages"
        );
        assert_eq!(
            usage_json(&result.usage),
            fixture["turn"]["usage"],
            "{name}: usage"
        );
    }

    /// The five-key turn usage shape (`_extract_usage`, `tasks.py:428-439`).
    fn usage_json(usage: &RunUsage) -> Value {
        serde_json::json!({
            "requests": usage.requests,
            "tool_calls": usage.tool_calls,
            "input_tokens": usage.input_tokens.to_json(),
            "total_tokens": usage.total_tokens().to_json(),
            "output_tokens": usage.output_tokens.to_json(),
        })
    }

    #[tokio::test]
    async fn text_openai_replays_capture_byte_for_byte() {
        one_turn_capture("text_openai").await;
    }

    #[tokio::test]
    async fn text_anthropic_replays_capture_byte_for_byte() {
        one_turn_capture("text_anthropic").await;
    }

    #[tokio::test]
    async fn tool_local_replays_capture_byte_for_byte() {
        one_turn_capture("tool_local").await;
    }

    #[tokio::test]
    async fn tool_anthropic_replays_capture_byte_for_byte() {
        one_turn_capture("tool_anthropic").await;
    }

    #[tokio::test]
    async fn tool_mcp_replays_capture_byte_for_byte() {
        one_turn_capture("tool_mcp").await;
    }

    #[tokio::test]
    async fn retry_validation_replays_capture_byte_for_byte() {
        one_turn_capture("retry_validation").await;
    }

    /// The stream items the handler sees, per capture.
    #[tokio::test]
    async fn stream_items_match_the_streamed_text() {
        let fixture = load_fixture("tool_local");
        let replies: Vec<_> = fixture["provider_exchanges"]
            .as_array()
            .unwrap()
            .iter()
            .map(reply_for)
            .collect();
        let outcome = run_capture(&fixture, 0, &[], replies).await;
        outcome.result.expect("tool_local runs");
        assert_eq!(
            outcome.events,
            vec![
                StreamItem::NonTextBoundary,
                StreamItem::TextStart {
                    initial: "Found 1 project.".to_owned()
                },
                StreamItem::End,
            ],
            "tool call first, then the text row, then the handler's finalize"
        );

        let fixture = load_fixture("text_openai");
        let replies = vec![reply_for(&fixture["provider_exchanges"][0])];
        let outcome = run_capture(&fixture, 0, &[], replies).await;
        outcome.result.expect("text_openai runs");
        let first = outcome.events.first().expect("events");
        assert!(
            matches!(first, StreamItem::TextStart { .. }),
            "a text-only response opens its row immediately: {first:?}"
        );
        assert_eq!(outcome.events.last(), Some(&StreamItem::End));
    }

    /// The usage-limit capture: 21 requests, the 21st tool call over
    /// the cap, ending `iteration_limit` with the pydantic-ai message.
    #[tokio::test]
    async fn usage_limit_stops_at_the_tool_call_cap() {
        let fixture = load_fixture("usage_limit");
        let exchanges = fixture["provider_exchanges"].as_array().unwrap();
        let replies: Vec<_> = exchanges.iter().map(reply_for).collect();
        let outcome = run_capture(&fixture, 0, &[], replies).await;
        assert_eq!(outcome.seen.len(), exchanges.len());
        for (index, seen) in outcome.seen.iter().enumerate() {
            assert_eq!(
                seen.body,
                request_bytes_of(&fixture["provider_exchanges"][index]),
                "usage_limit#{index}: request bytes"
            );
        }
        let error = outcome.result.expect_err("the cap fails the run");
        match error {
            TurnLoopError::UsageLimit(message) => {
                assert_eq!(message, fixture["turn"]["error_detail"]);
                assert!(matches!(
                    AgentError::from(TurnLoopError::UsageLimit(message.clone())),
                    AgentError::UsageLimit(_)
                ));
                assert_eq!(fixture["turn"]["error_code"], CODE_ITERATION_LIMIT);
            }
            other => panic!("expected a usage limit, got {other:?}"),
        }
    }

    /// The provider-error captures: the loop's message/status classify
    /// into the stored code.
    async fn provider_failure_capture(name: &str) {
        let fixture = load_fixture(name);
        let replies = vec![reply_for(&fixture["provider_exchanges"][0])];
        let outcome = run_capture(&fixture, 0, &[], replies).await;
        let error = outcome
            .result
            .err()
            .unwrap_or_else(|| panic!("{name}: the run fails"));
        let TurnLoopError::Provider {
            message,
            status_code,
        } = error
        else {
            panic!("{name}: expected a provider error");
        };
        let (code, detail) = classify_error(&message, status_code);
        assert_eq!(code, fixture["turn"]["error_code"], "{name}: code");
        assert_eq!(detail, fixture["turn"]["error_detail"], "{name}: detail");
    }

    #[tokio::test]
    async fn error_401_classifies_auth_failure() {
        provider_failure_capture("error_401").await;
    }

    #[tokio::test]
    async fn error_402_classifies_out_of_credit() {
        provider_failure_capture("error_402").await;
    }

    /// `replay_3turn`: three runs on one thread, each replaying the
    /// prior turns' stored blobs; every request matches its capture.
    #[tokio::test]
    async fn replay_3turn_replays_stored_history() {
        let fixture = load_fixture("replay_3turn");
        let turns = fixture["turns"].as_array().unwrap().clone();
        let mut history: Vec<Value> = Vec::new();
        for (index, turn) in turns.iter().enumerate() {
            let replies = vec![reply_for(&fixture["provider_exchanges"][index])];
            let outcome = run_capture(&fixture, index, &history, replies).await;
            assert_eq!(
                outcome.seen[0].body,
                request_bytes_of(&fixture["provider_exchanges"][index]),
                "replay_3turn#{index}: request bytes"
            );
            let result = outcome
                .result
                .unwrap_or_else(|error| panic!("replay_3turn#{index}: {error:?}"));
            assert_eq!(
                normalize_volatile(&dump_blob(&result.messages)),
                normalize_volatile(&turn["model_messages"]),
                "replay_3turn#{index}: new messages"
            );
            assert_eq!(
                usage_json(&result.usage),
                turn["turn"]["usage"],
                "replay_3turn#{index}: usage"
            );
            history.push(turn["model_messages"].clone());
        }
    }

    #[tokio::test]
    async fn limits_match_the_agent_construction() {
        let limits = turn_usage_limits();
        assert_eq!(limits.request_limit, Some(25));
        assert_eq!(limits.tool_calls_limit, Some(20));
        assert_eq!(limits.input_tokens_limit, None);
        assert_eq!(MAX_RETRIES, 2);
    }

    /// `for_run_step` keeps counts consecutive: a step whose failure
    /// set omits a name resets it.
    #[test]
    fn retry_counts_are_consecutive() {
        let mut retries = HashMap::new();
        retries.insert("search_issues".to_owned(), 1);
        let failed: HashSet<String> = HashSet::from(["list_issues".to_owned()]);
        let next = for_run_step(&retries, &failed);
        assert_eq!(next.get("list_issues"), Some(&1));
        assert_eq!(next.get("search_issues"), None);
    }

    /// `InstructionPart.join` (`messages.py:1771-1773`): parts separated
    /// by a blank line, the whole result stripped, `None` when empty.
    /// Production parts are already stripped (phase 6a), so the blank
    /// line is single; an unstripped part keeps its own whitespace.
    #[test]
    fn instructions_join_like_pydantic_ai() {
        let parts = vec!["base\n".to_owned(), "dynamic".to_owned()];
        assert_eq!(
            joined_instructions(&parts),
            Some("base\n\n\ndynamic".to_owned())
        );
        let stripped = vec!["base".to_owned(), "dynamic".to_owned()];
        assert_eq!(
            joined_instructions(&stripped),
            Some("base\n\ndynamic".to_owned())
        );
        assert_eq!(joined_instructions(&[]), None);
        assert_eq!(joined_instructions(&["  ".to_owned()]), None);
    }

    /// `TurnResponsePart` -> `TurnFinish` mapping for the five reasons
    /// the stream can carry.
    #[test]
    fn finish_reasons_map() {
        assert_eq!(finish_reason(Finish::Stop), TurnFinish::Stop);
        assert_eq!(finish_reason(Finish::Length), TurnFinish::Length);
        assert_eq!(finish_reason(Finish::ToolCall), TurnFinish::ToolCall);
        assert_eq!(
            finish_reason(Finish::ContentFilter),
            TurnFinish::ContentFilter
        );
    }

    /// `args_as_dict(raise_if_invalid=True)` (`messages.py:1838-1864`):
    /// absent/empty args read `{}`; an object parses; anything else is
    /// an error.
    #[test]
    fn tool_args_parse_like_python() {
        assert_eq!(args_as_dict_checked(&None).unwrap(), Map::new());
        assert_eq!(
            args_as_dict_checked(&Some(ToolArgs::Raw(String::new()))).unwrap(),
            Map::new()
        );
        let parsed = args_as_dict_checked(&Some(ToolArgs::Raw("{\"a\":1}".to_owned()))).unwrap();
        assert_eq!(parsed.get("a"), Some(&Value::Number(1.into())));
        assert!(args_as_dict_checked(&Some(ToolArgs::Raw("[1]".to_owned()))).is_err());
    }

    /// CPython `ensure_ascii` escaping for the content-filter body.
    #[test]
    fn ensure_ascii_matches_cpython() {
        assert_eq!(ensure_ascii("a\"b\\c\nd\te"), "a\\\"b\\\\c\\nd\\te");
        assert_eq!(ensure_ascii("—"), "\\u2014");
        assert_eq!(ensure_ascii("😀"), "\\ud83d\\ude00");
        assert_eq!(ensure_ascii("\u{1}"), "\\u0001");
    }
}
