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
//!
//! [`llm_config`] owns the BYOK LLM-config + title-generation HTTP shell
//! (PIDASHCONV-256) and [`stt_config`] the BYO speech-to-text config shell
//! (PIDASHCONV-256); [`common`] holds their shared request edge and
//! [`kms`] the production KMS wire. Sibling handler issues merge their
//! own routers into [`routes`]; merges keep both sides.

pub mod common;
pub mod kms;
pub mod llm_config;
pub mod perm;
pub mod stt_config;
pub mod throttles;

use axum::Router;

use crate::state::AppState;

/// Owned D-06 assistant routes (cutover granularity: registered paths
/// serve from Rust, everything else keeps proxying). Sibling handler
/// issues extend this merge; merges keep both sides.
pub fn routes() -> Router<AppState> {
    llm_config::routes().merge(stt_config::routes())
}
