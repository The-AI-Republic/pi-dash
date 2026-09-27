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
//! - Closed-source schema: Muse Code's JSONL schema is not publicly
//!   documented, so the parser is modeled on frames captured from a real
//!   binary (`runner/tests/fixtures/muse_code/`). Unmapped record types are
//!   preserved as `muse/<payload_type>`, but run setup fails loudly on a frame
//!   that is not a record envelope at all, so an upstream envelope change
//!   surfaces immediately instead of as a misleading crash at EOF.

pub mod bridge;
pub mod process;
pub mod schema;
