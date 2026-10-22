//! Translation from Pi Dash's user-facing [`ApprovalMode`] to the built-in
//! engine's per-thread posture, and to the runner's default decision for a
//! request the static policy does not otherwise cover.
//!
//! Two vocabularies meet here and the translation is where mistakes hide, so
//! it lives in one small, well-tested place:
//!
//! * The **engine** (codex app-server) takes a `sandbox` and an
//!   `approval_policy` string on `thread/start`. These decide what a command
//!   may do without asking and *when the engine surfaces a request to the
//!   runner at all*.
//! * The **runner** then evaluates each surfaced request against the static
//!   [`ApprovalPolicySection`](crate::config::schema::ApprovalPolicySection)
//!   (denylist / allowlist / auto-approve flags) and, for anything left
//!   undecided, the mode's default (see [`Policy`](crate::approval::policy)).
//!
//! ## Why every mode uses `approval_policy = "on-request"`
//!
//! The acceptance criterion "a denylisted command is refused regardless of
//! mode" requires the runner to stay in the loop even under full access: with
//! `approval_policy = "never"` the engine would run commands without ever
//! asking, and the denylist could not fire. So *all three modes* keep the
//! engine on `on-request` and let the runner auto-approve where the mode
//! allows. Full access therefore still shows the user no prompts (the runner
//! auto-approves everything the denylist does not refuse), matching today's
//! behaviour, while the denylist floor now holds in every mode.
//!
//! The codex `Bridge`'s own default — used by the cloud path, which passes no
//! mode — is left at the historical `(danger-full-access, never)` so cloud
//! chat is byte-for-byte unchanged; only a request that carries an explicit
//! mode gets the mediated posture below.

use crate::cloud::protocol::ApprovalMode;

/// The engine's per-thread posture: the two `thread/start` strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineThreadSettings {
    pub sandbox: &'static str,
    pub approval_policy: &'static str,
}

impl EngineThreadSettings {
    /// The codex `Bridge`'s historical hardcoded posture. Kept as the default
    /// for the mode-less (cloud) path so existing behaviour is unchanged.
    pub const LEGACY_FULL_ACCESS: Self = Self {
        sandbox: "danger-full-access",
        approval_policy: "never",
    };
}

impl Default for EngineThreadSettings {
    fn default() -> Self {
        Self::LEGACY_FULL_ACCESS
    }
}

/// Map a user-chosen [`ApprovalMode`] to the engine's per-thread posture.
///
/// The sandbox narrows with the mode; the approval policy is always
/// `on-request` so the runner mediates (see module docs).
pub fn engine_thread_settings(mode: ApprovalMode) -> EngineThreadSettings {
    match mode {
        ApprovalMode::Ask => EngineThreadSettings {
            sandbox: "read-only",
            approval_policy: "on-request",
        },
        ApprovalMode::Workspace => EngineThreadSettings {
            sandbox: "workspace-write",
            approval_policy: "on-request",
        },
        ApprovalMode::FullAccess => EngineThreadSettings {
            sandbox: "danger-full-access",
            approval_policy: "on-request",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_mode_is_read_only_and_on_request() {
        let s = engine_thread_settings(ApprovalMode::Ask);
        assert_eq!(s.sandbox, "read-only");
        assert_eq!(s.approval_policy, "on-request");
    }

    #[test]
    fn workspace_mode_is_workspace_write_and_on_request() {
        let s = engine_thread_settings(ApprovalMode::Workspace);
        assert_eq!(s.sandbox, "workspace-write");
        assert_eq!(s.approval_policy, "on-request");
    }

    #[test]
    fn full_access_mode_keeps_engine_in_the_loop() {
        // Full access must NOT be `never`: the runner has to see every request
        // so the denylist can refuse it. No user prompts still result because
        // the runner auto-approves everything the denylist does not decline.
        let s = engine_thread_settings(ApprovalMode::FullAccess);
        assert_eq!(s.sandbox, "danger-full-access");
        assert_eq!(s.approval_policy, "on-request");
        assert_ne!(s.approval_policy, "never");
    }

    #[test]
    fn default_matches_legacy_full_access_posture() {
        // The mode-less (cloud) path keeps the historical hardcoded posture.
        assert_eq!(EngineThreadSettings::default().sandbox, "danger-full-access");
        assert_eq!(EngineThreadSettings::default().approval_policy, "never");
    }

    #[test]
    fn approval_mode_defaults_to_full_access() {
        assert_eq!(ApprovalMode::default(), ApprovalMode::FullAccess);
    }
}
