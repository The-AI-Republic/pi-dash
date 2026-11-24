//! Antigravity integration. Drives Google's Antigravity CLI coding agent
//! over the Agent Client Protocol (ACP) by spawning its native ACP server
//! (`agy acp`) and speaking JSON-RPC to it directly on stdin/stdout,
//! translating the emitted ACP traffic into the agent-agnostic
//! [`crate::agent::BridgeEvent`] shape used by the daemon.
//!
//! The ACP driver itself lives in [`crate::acp`] (shared with Grok); this
//! module only carries Antigravity's profile: its spawn argv
//! (`agy acp --dangerously-skip-permissions --disable-telemetry --log-file
//! <runner log dir>` in the run's working directory), its model posture (the
//! configured model is applied through the Model session option the server
//! advertises, set via `session/set_config_option`), its interrupt posture
//! (the protocol-level `session/cancel` notification), and its resume
//! posture (a real `session/resume` — no history replay — when the server
//! advertises it, which `agy acp` does).
//!
//! Auth: the main supported path is the user's own interactive `agy`
//! sign-in, cached in the OS keychain, which `agy` started by the runner
//! (running as the user's own service) can read. `GEMINI_API_KEY` plus
//! `"modelProvider": "gemini"` in `~/.gemini/antigravity-cli/settings.json`,
//! or `AGY_ADC_AUTH=true`, is the headless fallback. The pre-pickup presence
//! check is NEWPIDASH-397; until it lands,
//! [`crate::agent::auth::has_antigravity_credentials`] returns `true` (never
//! block on a guess). The run-time safety net lives in the shared bridge: a
//! turn that fails with the ACP authentication-required error (code `-32000`)
//! surfaces as `agent_auth_missing` with the Antigravity fix hint.
//!
//! The user's `~/.gemini/antigravity-cli/settings.json` is shared with their
//! interactive `agy` and is never edited — the bridge passes flags instead.

use std::path::Path;

use crate::acp::AcpProfile;

/// Antigravity's ACP profile: model via session config options, interrupt via
/// the `session/cancel` notification, resume via `session/resume`.
pub const PROFILE: AcpProfile = AcpProfile {
    label: "antigravity",
    send_model_on_session_new: false,
    apply_model_via_config_option: true,
    cancel_via_notification: true,
    auth_fix_hint: crate::agent::auth::ANTIGRAVITY_FIX_HINT,
};

/// Argv (after the binary) that starts Antigravity's ACP server. `log_dir`
/// is the runner's log directory; when known the server log goes to
/// `<log_dir>/agy-acp.log` instead of the default
/// `~/.gemini/antigravity-cli/log/acp-*.log`.
pub fn argv(log_dir: Option<&Path>) -> Vec<String> {
    let mut argv = vec![
        "acp".to_string(),
        "--dangerously-skip-permissions".to_string(),
        "--disable-telemetry".to_string(),
    ];
    if let Some(dir) = log_dir {
        argv.push("--log-file".to_string());
        argv.push(dir.join("agy-acp.log").to_string_lossy().into_owned());
    }
    argv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn antigravity_argv_matches_documented_flags() {
        let got = argv(Some(Path::new("/var/log/runner")));
        assert_eq!(
            got,
            vec![
                "acp",
                "--dangerously-skip-permissions",
                "--disable-telemetry",
                "--log-file",
                "/var/log/runner/agy-acp.log",
            ]
        );
        // No log dir: the flag pair is omitted, never dangling.
        assert_eq!(
            argv(None),
            vec![
                "acp",
                "--dangerously-skip-permissions",
                "--disable-telemetry",
            ]
        );
    }
}
