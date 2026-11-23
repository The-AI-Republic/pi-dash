//! Assistant pure layer (D-06, stage 5).
//!
//! Ports the DB-free bottom of `apps/api/pi_dash/assistant/`:
//!
//! * [`serializers`] — `serializers.py:1-162` (all four serializers: output
//!   DTOs plus the pure validation rules, including the write-only
//!   encrypted inputs that never render back).
//! * [`errors`] — `errors.py:15-107` (`AssistantError` base + 13 subclasses
//!   + `MAX_THREAD_MESSAGES` / `MAX_MESSAGE_CHARS`).
//! * [`surface`] — the assistant surface enum (NEWPIDASH-392): where a
//!   model choice is used (web assistant, desktop assistant, Pi Dash Agent).
//!
//! Fixture ids replayed by the unit tests alongside each module:
//! `rust-api/fixtures/assistant/serializers/*.golden.json` (F-A6-01) and
//! `rust-api/fixtures/assistant/errors.json` (F-A6-02).

pub mod errors;
pub mod serializers;
pub mod surface;

pub use errors::{AssistantError, MAX_MESSAGE_CHARS, MAX_THREAD_MESSAGES};
pub use serializers::{
    has_active_turn, validate_api_key, validate_base_url, validate_llm_config, validate_mcp_name,
    validate_mcp_url, validate_model_name, validate_stt_config, LlmConfigView, McpServerView,
    SttConfigView, ThreadView,
};
pub use surface::Surface;
