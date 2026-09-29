//! Assistant domain guards (D-06, stage 5).
//!
//! Ports the DB-free guard half of `apps/api/pi_dash/assistant/`:
//!
//! * [`crypto`] — `crypto.py:1-233` (PIDASHCONV-248).
//! * [`ssrf`] — `ssrf.py:1-43` (PIDASHCONV-248).
//! * [`markdown`] — `runtime/markdown.py:1-38` (PIDASHCONV-248).
//! * [`llm`] — `runtime/llm.py:1-115` (PIDASHCONV-250).
//! * [`title`] — `runtime/title.py:1-147` (PIDASHCONV-250).
//! * [`agent`] — `runtime/deps.py`, `runtime/agent.py`,
//!   `runtime/instructions.py` (PIDASHCONV-250).

pub mod agent;
pub mod crypto;
pub mod llm;
pub mod markdown;
pub mod ssrf;
pub mod title;
