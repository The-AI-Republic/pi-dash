//! Live assistant turn runtime (PIDASHCONV-1110, stage 8).
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
//! * Provider streaming (PIDASHCONV-1111), the MCP client (1112), local
//!   tools (1113), the agent loop, and the live seam (863) land behind
//!   the same `turn` root; each cites its Python source at the top.
//!
//! Reuse rule: foundation pieces that are `pub` and constructible from
//! the binary are reused (`Lane`, `ToolCall`, usage, `KeyResolver`,
//! the services decision surfaces). The D-11 history/request-builder
//! types are not (their struct-variant fields are crate-private), so
//! the turn message model lives here, pinned against the same
//! pydantic-ai sources and the
//! `fixtures/assistant/turn-runtime/` captures.
//!
//! Local tools layer (PIDASHCONV-1113): the 17 local tool schemas +
//! executors (`assistant/tools/*.py` port). Each layer merges as its own
//! tested module; PIDASHCONV-863 wires them into `worker()` (agent loop,
//! live seam, parity flip, replay proof). Until then this tree is
//! exercised by its unit tests only.

pub mod history;
pub mod tool_args;
pub mod tool_schemas;
pub mod tools_local;
pub mod tools_local_store;
