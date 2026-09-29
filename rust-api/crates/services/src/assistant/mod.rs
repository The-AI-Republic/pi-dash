//! Assistant domain guards (D-06, stage 5).
//!
//! Ports the DB-free guard half of `apps/api/pi_dash/assistant/`:
//!
//! * [`crypto`] — `crypto.py:1-233` (PIDASHCONV-248).
//! * [`ssrf`] — `ssrf.py:1-43` (PIDASHCONV-248).
//! * [`markdown`] — `runtime/markdown.py:1-38` (PIDASHCONV-248).

pub mod crypto;
pub mod markdown;
pub mod ssrf;
