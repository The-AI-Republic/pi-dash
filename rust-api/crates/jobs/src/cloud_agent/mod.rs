//! Cloud agent: the chat engine the assistant runs on (agent naming P2).
//!
//! The multi-tenant, cloud-based agent engine: model routing, the
//! model loop ([`agent`]), provider streaming ([`streaming`]),
//! tool-call handling, usage limits ([`usage`]), and provider error
//! classification ([`errors`]). It answers the assistant's chat in
//! the cloud (web) version; it is not a runner and never executes an
//! issue run. Assistant code (threads, turns, tools, persistence)
//! imports this engine; the engine imports nothing from the
//! assistant.

pub mod agent;
pub mod errors;
pub mod history;
pub mod keys;
pub mod provider;
#[cfg(test)]
pub mod replay;
pub mod streaming;
pub mod usage;
pub mod usage_table;
pub mod validation;

// ---------------------------------------------------------------------------
// The engine's boundary types (moved from assistant::run_turn in P2;
// run_turn re-exports them so existing paths keep compiling).
// ---------------------------------------------------------------------------

/// One inbound agent-stream item, mirroring the `pydantic_ai` events
/// `_Streamer.handle` matches on (`tasks.py:228-245`): a `TextPart` start
/// (with its initial content, possibly empty), a `TextPartDelta` chunk, any
/// other part boundary (closes the open row without opening a new one), or
/// the end of the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamItem {
    TextStart { initial: String },
    TextDelta { chunk: String },
    NonTextBoundary,
    End,
}

/// Agent-run failure, mirroring the three `except` arms of `_run_turn`
/// (`tasks.py:346-361`) plus the output-limit check inside
/// `_classify_error` (`tasks.py:489-499`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentError {
    /// `UsageLimitExceeded` → `iteration_limit` (`tasks.py:351-355`).
    UsageLimit(String),
    /// A typed `AssistantError` raised inside the run — by a model adapter
    /// or an ee provider — already carrying its stable code and user-facing
    /// detail. Used verbatim, never text-guessed: the typed-first branch of
    /// `_classify_error` (`tasks.py:453-457`, #1126, kept by #1430).
    ///
    /// Python walks `exc.__cause__` because the agent runtime may re-raise a
    /// provider failure wrapped in its own type. Rust has no exception chain:
    /// the seam implementor performs that walk when converting its native
    /// error into an [`AgentError`] at the boundary, emitting `Typed` when
    /// any cause is an `AssistantError` (see `assistant::run_turn::ModelError`
    /// for the same already-classified crossing at model-resolution time).
    Typed { code: String, detail: String },
    /// The model ran out of output tokens (`IncompleteToolCall`, or the
    /// length-exhaustion `UnexpectedModelBehavior`) → `response_truncated`
    /// with the constant cut-off detail: the `_hit_output_limit` branch
    /// (`tasks.py:489-499`), which Django checks after the provider
    /// classifier and before `internal`. The variant bypasses
    /// [`errors::classify_provider_error`] outright — neither pydantic-ai message matches
    /// any provider marker, so the bypass classifies identically.
    OutputTruncated,
    /// Any other provider failure → `assistant::run_turn::TurnSeam::turn_error_for_user`
    /// (`tasks.py:356-361`). `message` is `str(exc)`; `status_code` the
    /// exception's HTTP status when it carries one; `body_detail` the
    /// provider's own one-line explanation from the structured response
    /// body (`""` when it gave none); `model_name` the model the failing
    /// call used (`""` when unknown).
    Provider {
        message: String,
        status_code: Option<u16>,
        body_detail: String,
        model_name: String,
    },
}
