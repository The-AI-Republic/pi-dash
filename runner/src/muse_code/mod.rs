//! Muse Code integration. Drives Meta's `muse exec --json --prompt-file
//! <PATH>` headless one-shot mode and translates the emitted JSONL events into
//! the agent-agnostic [`crate::agent::BridgeEvent`] shape used by the daemon.
//!
//! MVP limitations (tracked as follow-ups):
//! - Approvals bypass: runs with `--yolo` (disable approval prompts and
//!   sandbox). Wiring a real approval prompt is out of scope for the first
//!   pass, mirroring the Cursor bridge's `--force` posture.
//! - One-shot per turn: `muse exec` reads the prompt from a file and runs the
//!   turn to completion, so each turn spawns a fresh subprocess (reusing the
//!   prior `--session-id` for continuity) rather than feeding turns over stdin.
//! - Closed-source schema: Muse Code's JSONL event schema is not fully
//!   documented publicly; the schema layer is tolerant (envelope + retained
//!   raw body + an `Unknown` catch-all), so upstream drift degrades to
//!   "preserved but unmapped" rather than a bridge crash.

pub mod bridge;
pub mod process;
pub mod schema;
