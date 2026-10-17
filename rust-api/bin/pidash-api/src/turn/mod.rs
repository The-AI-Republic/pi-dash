//! Live assistant turn runtime (PIDASHCONV-863, stage 8).
//!
//! The provider-backed half of `assistant.run_turn`: the pieces the
//! `TurnSeam` boundary (`pidash_jobs::assistant::run_turn`) left to the
//! handler layer. The drive (`drive_turn`), the streaming sink, the SQL
//! shapes, and the error classifier live in the jobs crate; everything
//! here is the live wiring behind the seam:
//!
//! * [`history`] — the `model_messages` codec: pydantic-ai 1.107
//!   `ModelMessage` JSON <-> the loop's message model, plus the
//!   retry-feedback renderer for replayed validation errors.
//! * Provider streaming, MCP transport, local tools, the agent loop,
//!   and the live seam land in later phases behind the same `turn`
//!   root; each cites its Python source at the top.
//!
//! Reuse rule: foundation pieces that are `pub` and constructible from
//! the binary are reused (`Lane`, `ToolCall`, usage, `KeyResolver`,
//! the services decision surfaces). The D-11 history/request-builder
//! types are not (their struct-variant fields are crate-private), so
//! the turn message model and the streaming request builders live here,
//! pinned against the same pydantic-ai sources and the
//! `fixtures/assistant/turn-runtime.json` captures.

pub mod history;
