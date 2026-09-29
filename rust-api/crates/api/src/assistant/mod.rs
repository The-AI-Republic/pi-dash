//! Assistant permission + throttle surface (D-06, stage 5).
//!
//! Ports the permission closure of `apps/api/pi_dash/assistant/`:
//!
//! * [`perm`] — `views/_base.py:1-34` (member gate, owned-thread scope)
//!   plus the SSE resolve matrix in `views/events.py:28-41`.
//! * [`throttles`] — the six `UserRateThrottle` subclasses in
//!   `views/messages.py:29-42`, `views/llm_config.py:81,111`,
//!   `views/stt_config.py:84`, `views/transcribe.py:57`,
//!   `views/agent_profile.py:67` with rates from
//!   `pi_dash/settings/common.py:93-117`.
//!
//! Fixture id F-A6-07 (`rust-api/fixtures/assistant/perms.json`).
//! Shape of the port: pure decision logic over already-fetched rows. Role
//! fetching (`WorkspaceMember` lookup) and thread fetching stay with the
//! handler layer, which owns the only scoped database handle (tenancy rule);
//! this module decides and renders, exactly like the Python view helpers.

pub mod perm;
pub mod throttles;
