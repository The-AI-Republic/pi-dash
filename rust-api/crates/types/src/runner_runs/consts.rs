//! Shared runner/run/chat constants (D-15, stage 5).
//!
//! Port of the module-level constants the run/chat views and services
//! share (all under `apps/api/pi_dash/runner/`):
//!
//! * `DEFAULT_PER_PAGE` / `MAX_PER_PAGE` (`views/runs.py:36-37`) →
//!   [`DEFAULT_PER_PAGE`] / [`MAX_PER_PAGE`].
//! * `MAX_EVENT_PAYLOAD_BYTES` (`views/run_endpoints.py:45`) →
//!   [`MAX_EVENT_PAYLOAD_BYTES`].
//! * Stream-ticket store (`views/run_endpoints.py:491-503`) →
//!   [`WS_UPGRADE_TICKET_KEY_PREFIX`], [`WS_UPGRADE_TICKET_TTL_SECS`],
//!   [`WS_UPGRADE_TICKET_EXPIRES_IN_SECS`],
//!   [`WS_UPGRADE_TICKET_PAYLOAD_KEYS`], [`ws_upgrade_ticket_key`].
//! * `CHAT_EVENT_PAYLOAD_MAX_BYTES` (`views/chat.py:64`) →
//!   [`CHAT_EVENT_PAYLOAD_MAX_BYTES`].
//! * `CHAT_EVENT_CHANNEL_PREFIX` / `CHAT_ACTIVE_TIMEOUT_SECS`
//!   (`services/chat.py:40-41`) → [`CHAT_EVENT_CHANNEL_PREFIX`] /
//!   [`CHAT_ACTIVE_TIMEOUT_SECS`]; `event_channel` (`:44-45`) →
//!   [`event_channel`].
//! * `RUN_MESSAGE_DEDUPE_TTL_SECS` default (`tasks.py:208,219`) →
//!   [`RUN_MESSAGE_DEDUPE_TTL_SECS_DEFAULT`] +
//!   [`run_message_dedupe_ttl_secs`].
//! * `ACTIVE_RUN_STATUSES` (`views/metrics.py:40-46`) →
//!   [`ACTIVE_RUN_STATUSES`].
//!
//! Translation notes:
//!
//! * Counts/seconds are `i64`, as the paginator kernels take them; byte
//!   sizes are `usize` for direct `len()` comparison.
//! * The dedupe TTL is settings-backed in Python
//!   (`getattr(settings, …, 604800)`); this crate performs no I/O, so the
//!   caller passes the configured value in (license-port callback
//!   precedent).
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-01-types-pure.golden.json`
//! (FX-RUN-01 `consts`).
//!
//! Ported bugs: none found in these units on read-through.

use super::enums::AgentRunStatus;

/// Runs-list page size fallback (`views/runs.py:36`).
pub const DEFAULT_PER_PAGE: i64 = 30;
/// Runs-list page size ceiling (`views/runs.py:37`).
pub const MAX_PER_PAGE: i64 = 200;

/// Daemon event-batch payload cap (`views/run_endpoints.py:45`): 64 KiB.
pub const MAX_EVENT_PAYLOAD_BYTES: usize = 64 * 1024;

/// Chat event payload cap (`views/chat.py:64`): 256 KiB.
pub const CHAT_EVENT_PAYLOAD_MAX_BYTES: usize = 256 * 1024;

/// Redis channel prefix for chat session events (`services/chat.py:40`).
pub const CHAT_EVENT_CHANNEL_PREFIX: &str = "agent_chat_session:";
/// Idle seconds before an active chat turn is swept (`services/chat.py:41`).
pub const CHAT_ACTIVE_TIMEOUT_SECS: i64 = 1800;

/// Chat event channel for a session (`services/chat.py:44-45`).
pub fn event_channel(session_id: &str) -> String {
    format!("{CHAT_EVENT_CHANNEL_PREFIX}{session_id}")
}

/// Default idempotency-row TTL when the setting is unset
/// (`tasks.py:208,219`): 7 days.
pub const RUN_MESSAGE_DEDUPE_TTL_SECS_DEFAULT: i64 = 604800;

/// Resolve the idempotency-row TTL: the configured setting, or the
/// 7-day default when unset.
pub fn run_message_dedupe_ttl_secs(configured: Option<i64>) -> i64 {
    configured.unwrap_or(RUN_MESSAGE_DEDUPE_TTL_SECS_DEFAULT)
}

/// Redis key prefix for WS upgrade tickets (`views/run_endpoints.py:493`).
pub const WS_UPGRADE_TICKET_KEY_PREFIX: &str = "ws_upgrade_ticket:";
/// Ticket store expiry (`views/run_endpoints.py:502`, `ex=60`).
pub const WS_UPGRADE_TICKET_TTL_SECS: i64 = 60;
/// Ticket lifetime reported to the caller (`views/run_endpoints.py:508`).
pub const WS_UPGRADE_TICKET_EXPIRES_IN_SECS: i64 = 60;
/// Ticket payload keys (`views/run_endpoints.py:494-499`), in order.
pub const WS_UPGRADE_TICKET_PAYLOAD_KEYS: [&str; 4] =
    ["run_id", "stream", "runner_id", "expires_at"];

/// Redis key for a WS upgrade ticket (`views/run_endpoints.py:493`).
pub fn ws_upgrade_ticket_key(ticket: &str) -> String {
    format!("{WS_UPGRADE_TICKET_KEY_PREFIX}{ticket}")
}

/// In-flight statuses counted by `pi_dash_runs_active`
/// (`views/metrics.py:40-46`), in tuple order. Distinct from
/// `is_active` (which also covers `QUEUED`/`WAITING_FOR_WORKTREE`).
pub const ACTIVE_RUN_STATUSES: [AgentRunStatus; 5] = [
    AgentRunStatus::Assigned,
    AgentRunStatus::Running,
    AgentRunStatus::CancelRequested,
    AgentRunStatus::AwaitingApproval,
    AgentRunStatus::AwaitingReauth,
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-01-types-pure.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn consts(fx: &Value) -> &serde_json::Map<String, Value> {
        fx.get("consts")
            .and_then(Value::as_object)
            .expect("consts section")
    }

    #[test]
    fn scalar_consts_match_fixture() {
        let fx = fixture();
        let c = consts(&fx);
        assert_eq!(
            c.get("DEFAULT_PER_PAGE").and_then(Value::as_i64),
            Some(DEFAULT_PER_PAGE)
        );
        assert_eq!(
            c.get("MAX_PER_PAGE").and_then(Value::as_i64),
            Some(MAX_PER_PAGE)
        );
        assert_eq!(
            c.get("MAX_EVENT_PAYLOAD_BYTES").and_then(Value::as_u64),
            Some(MAX_EVENT_PAYLOAD_BYTES as u64)
        );
        assert_eq!(
            c.get("CHAT_EVENT_PAYLOAD_MAX_BYTES")
                .and_then(Value::as_u64),
            Some(CHAT_EVENT_PAYLOAD_MAX_BYTES as u64)
        );
        assert_eq!(
            c.get("CHAT_EVENT_CHANNEL_PREFIX").and_then(Value::as_str),
            Some(CHAT_EVENT_CHANNEL_PREFIX)
        );
        assert_eq!(
            c.get("CHAT_ACTIVE_TIMEOUT_SECS").and_then(Value::as_i64),
            Some(CHAT_ACTIVE_TIMEOUT_SECS)
        );
        assert_eq!(
            c.get("RUN_MESSAGE_DEDUPE_TTL_SECS_default")
                .and_then(Value::as_i64),
            Some(RUN_MESSAGE_DEDUPE_TTL_SECS_DEFAULT)
        );
        assert_eq!(DEFAULT_PER_PAGE, 30);
        assert_eq!(MAX_PER_PAGE, 200);
        assert_eq!(MAX_EVENT_PAYLOAD_BYTES, 65_536);
        assert_eq!(CHAT_EVENT_PAYLOAD_MAX_BYTES, 262_144);
        assert_eq!(CHAT_ACTIVE_TIMEOUT_SECS, 1800);
        assert_eq!(RUN_MESSAGE_DEDUPE_TTL_SECS_DEFAULT, 604_800);
    }

    #[test]
    fn stream_ticket_consts_match_fixture() {
        let fx = fixture();
        let ticket = consts(&fx).get("stream_ticket").expect("stream_ticket");
        assert_eq!(
            ticket.get("key").and_then(Value::as_str),
            Some("ws_upgrade_ticket:{ticket}"),
            "key template shape",
        );
        assert_eq!(ws_upgrade_ticket_key("abc"), "ws_upgrade_ticket:abc");
        assert_eq!(
            ticket.get("ex_secs").and_then(Value::as_i64),
            Some(WS_UPGRADE_TICKET_TTL_SECS)
        );
        assert_eq!(
            ticket.get("expires_in_secs").and_then(Value::as_i64),
            Some(WS_UPGRADE_TICKET_EXPIRES_IN_SECS)
        );
        let keys: Vec<String> = WS_UPGRADE_TICKET_PAYLOAD_KEYS
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            ticket.get("payload_keys"),
            Some(&Value::Array(keys.into_iter().map(Value::String).collect()))
        );
    }

    #[test]
    fn event_channel_matches_fixture_shape() {
        let fx = fixture();
        let channel = consts(&fx).get("event_channel").expect("event_channel");
        assert_eq!(
            channel.get("example").and_then(Value::as_str),
            Some("agent_chat_session:<session_id>")
        );
        assert_eq!(
            event_channel("<session_id>"),
            "agent_chat_session:<session_id>"
        );
        assert_eq!(event_channel("9f3b"), "agent_chat_session:9f3b");
    }

    #[test]
    fn dedupe_ttl_resolves_default() {
        assert_eq!(run_message_dedupe_ttl_secs(None), 604_800);
        assert_eq!(run_message_dedupe_ttl_secs(Some(60)), 60);
    }

    #[test]
    fn metrics_active_set_matches_views_order() {
        let values: Vec<&str> = ACTIVE_RUN_STATUSES.iter().map(|s| s.value()).collect();
        assert_eq!(
            values,
            [
                "assigned",
                "running",
                "cancel_requested",
                "awaiting_approval",
                "awaiting_reauth"
            ]
        );
    }
}
