//! Grok integration. Drives xAI's Grok CLI coding agent over the Agent
//! Client Protocol (ACP) by spawning its native ACP server (`grok agent
//! stdio`) and speaking JSON-RPC to it directly on stdin/stdout, translating
//! the emitted ACP traffic into the agent-agnostic
//! [`crate::agent::BridgeEvent`] shape used by the daemon.
//!
//! The ACP driver itself lives in [`crate::acp`] (shared with Antigravity);
//! this module only carries Grok's profile: its spawn argv, its model
//! posture (the configured model rides as the extra `model` field on
//! `session/new`), and its interrupt posture (OS signal — Grok cleans up its
//! ACP session on SIGINT). A known resume session id is kept as the run's
//! thread-id seed only; Grok's server advertises no session revival, so no
//! prior conversation state is restored via `session/load`.

use crate::acp::AcpProfile;

/// Grok's ACP profile: model on `session/new`, interrupt by OS signal, no
/// session revival and no config-option application (unchanged from the
/// pre-shared bridge).
pub const PROFILE: AcpProfile = AcpProfile {
    label: "grok",
    send_model_on_session_new: true,
    apply_model_via_config_option: false,
    cancel_via_notification: false,
    auth_fix_hint: crate::agent::auth::GROK_FIX_HINT,
};

/// Argv (after the binary) that starts Grok's native ACP server.
pub fn argv() -> Vec<String> {
    vec!["agent".to_string(), "stdio".to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grok_profile_preserves_legacy_posture() {
        // Copy to a local so the assertions exercise the value rather than
        // tripping `assertions_on_constants`.
        let profile = PROFILE;
        assert_eq!(profile.label, "grok");
        assert!(profile.send_model_on_session_new);
        assert!(!profile.apply_model_via_config_option);
        assert!(!profile.cancel_via_notification);
    }

    #[test]
    fn grok_argv_starts_the_acp_server() {
        assert_eq!(argv(), vec!["agent".to_string(), "stdio".to_string()]);
    }
}
