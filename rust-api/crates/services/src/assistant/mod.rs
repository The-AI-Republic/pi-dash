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
//! * [`mcp`] — `runtime/mcp.py:1-309` (PIDASHCONV-251).
//! * [`seams`] — `ee/assistant/model_provider.py` + `stt_provider.py`
//!   (PIDASHCONV-251).
//! * [`tools_scoping`] — `tools/_scoping.py:1-103` (PIDASHCONV-252).
//! * [`tools_results`] — `tools/_results.py:1-69` (PIDASHCONV-252).
//! * [`tools_comments`] — `tools/comments.py:1-77` (PIDASHCONV-252).
//! * [`tools_projects`] — `tools/projects.py:1-65` (PIDASHCONV-252).
//! * [`tools_runs`] — `tools/runs.py:1-96` (PIDASHCONV-252).

pub mod agent;
pub mod crypto;
pub mod llm;
pub mod markdown;
pub mod mcp;
pub mod seams;
pub mod ssrf;
pub mod title;
pub mod tools_comments;
pub mod tools_projects;
pub mod tools_results;
pub mod tools_runs;
pub mod tools_scoping;
