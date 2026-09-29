//! Redis touch points of the assistant thread surface (D-06, stage 5).
//!
//! Three behaviors of `apps/api/pi_dash/assistant/views/` need a Redis
//! client in the API process:
//!
//! * `messages.py:125` / `threads.py:107` — the cancel signal:
//!   `SET assistant:cancel:<turn_id> "1" EX 600`, every failure swallowed.
//! * `messages.py:39-42` + `throttles.rs` — the message-POST brake
//!   (30/hour): DRF's sliding-window decision over the cached timestamp
//!   history (`throttle_assistant_message_<user_pk>`, timeout = window).
//! * `events.py:77-79` — the SSE live tail: `SUBSCRIBE
//!   assistant:thread:<thread_id>` after the replay prefix.
//!
//! No `redis` crate exists anywhere in the workspace yet, and both
//! `api/Cargo.toml` and `api/src/state.rs` (where a shared handle would
//! live) are read-only foundation files — so the transport half is a
//! dedicated foundation issue (filed by PIDASHCONV-255, which waits on it).
//! This module pins everything the transport will execute — key formats,
//! expiries, and the throttle cache algorithm over the ported
//! [`crate::assistant::throttles`] pure functions — so the foundation run
//! only supplies the client, and every call site below already reads like
//! the finished wiring.
//!
//! Interim behavior (until the foundation issue lands): cancel still answers
//! 204 (exactly like Python's swallow-everything `except`), the throttle
//! allows (fresh-user contract traffic never trips it), and the SSE stream
//! serves the replay prefix plus `1s` keepalives (the live tail the
//! subscription would feed is the only gap). Each site is marked PENDING
//! with the foundation issue id; nothing here merges until they are wired.

use uuid::Uuid;

use crate::assistant::throttles::{cache_key, MESSAGE_THROTTLE};
use crate::state::AppState;

/// Cancel-signal write (`tasks.py:54-55`, `messages.py:125`,
/// `threads.py:107`): `SET <cancel_key> "1" EX 600`. Returns the exact
/// `(key, value, expiry_secs)` the transport must send; failures are
/// swallowed by the caller, exactly like the Python `except Exception: pass`.
pub fn cancel_set_command(turn_id: &Uuid) -> (String, String, u64) {
    (
        pidash_jobs::assistant::cancel_key(turn_id),
        "1".to_owned(),
        600,
    )
}

/// Signal an in-flight turn to stop (`messages.py:124-128`,
/// `threads.py:105-110`). PENDING(redis-foundation): sends
/// [`cancel_set_command`] through the shared client and swallows every
/// failure; until the transport lands this is a documented no-op that
/// preserves the 204 response shape.
pub async fn signal_cancel(_state: &AppState, turn_id: &Uuid) {
    let (key, _value, _expiry_secs) = cancel_set_command(turn_id);
    // PENDING(redis-foundation): `SET key "1" EX 600`, swallow all errors.
    tracing::debug!(key = key.as_str(), "assistant.cancel: redis unavailable; signal deferred");
}

/// Throttle-cache key for a message POST
/// (`SimpleRateThrottle.get_cache_key`: `throttle_<scope>_<ident>`, where
/// `ident` is the authenticated user's pk).
pub fn message_throttle_key(user_id: &Uuid) -> String {
    cache_key(MESSAGE_THROTTLE.scope, &user_id.to_string())
}

/// Verdict of the message-POST brake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleVerdict {
    Allow,
    Deny,
}

/// Evaluate the message-POST throttle (`messages.py:39-42`): trim history
/// entries `<= now - duration`, allow iff fewer than `num_requests` remain
/// ([`crate::assistant::throttles::allow_request`]); on allow the caller
/// records `now` at the front and re-caches with timeout `duration`
/// (here [`MESSAGE_THROTTLE`]'s 3600s window). PENDING(redis-foundation):
/// the history fetch/store goes through the shared client; until the
/// transport lands callers allow (fresh-user contract traffic never trips
/// the 30/hour brake) and this function documents the exact decision the
/// transport will execute.
pub fn evaluate_message_throttle(history: &[f64], now: f64) -> ThrottleVerdict {
    if crate::assistant::throttles::allow_request(
        history,
        now,
        MESSAGE_THROTTLE.requests,
        MESSAGE_THROTTLE.window_secs,
    ) {
        ThrottleVerdict::Allow
    } else {
        ThrottleVerdict::Deny
    }
}

/// Check the message-POST brake for `user_id`. PENDING(redis-foundation):
/// always allows until the throttle cache is wired (see
/// [`evaluate_message_throttle`] for the decision the transport executes).
pub async fn check_message_throttle(_state: &AppState, _user_id: &Uuid) -> ThrottleVerdict {
    // PENDING(redis-foundation): fetch history at `message_throttle_key`,
    // evaluate, record `now`, re-cache with 3600s timeout; deny with the
    // `Throttled` 429 body on quota exhaustion.
    ThrottleVerdict::Allow
}

/// SSE keepalive frame (`events.py:81-84`): one `get_message(timeout=1.0)`
/// miss yields `": keepalive\n\n"`.
pub const SSE_KEEPALIVE_FRAME: &str = ": keepalive\n\n";

/// Redis channel the SSE live tail subscribes to after the replay prefix
/// (`events.py:77-79`): `assistant:thread:<thread_id>` (same builder the
/// worker publishes through, `db::assistant::event_queries::event_channel`).
pub fn live_tail_channel(thread_id: &Uuid) -> String {
    pidash_db::assistant::event_queries::event_channel(&thread_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_command_pins_key_value_and_expiry() {
        let turn: Uuid = "db68f428-df63-4de8-b060-2d4038a5b1f4".parse().expect("uuid");
        assert_eq!(
            cancel_set_command(&turn),
            (
                "assistant:cancel:db68f428-df63-4de8-b060-2d4038a5b1f4".to_owned(),
                "1".to_owned(),
                600,
            )
        );
    }

    #[test]
    fn throttle_key_uses_drf_cache_format() {
        let user: Uuid = "a7107351-38ac-4f3d-91d5-ee34c2459bd6".parse().expect("uuid");
        assert_eq!(
            message_throttle_key(&user),
            "throttle_assistant_message_a7107351-38ac-4f3d-91d5-ee34c2459bd6"
        );
    }

    #[test]
    fn throttle_trips_at_thirty_per_hour() {
        let now = 1_700_000_000.0;
        let history: Vec<f64> = (0..30).map(|i| now - f64::from(i) * 60.0).collect();
        assert_eq!(evaluate_message_throttle(&history, now), ThrottleVerdict::Deny);
        assert_eq!(
            evaluate_message_throttle(&history[1..], now),
            ThrottleVerdict::Allow
        );
    }

    #[test]
    fn live_tail_channel_matches_publish_channel() {
        let thread: Uuid = "db68f428-df63-4de8-b060-2d4038a5b1f4".parse().expect("uuid");
        assert_eq!(
            live_tail_channel(&thread),
            "assistant:thread:db68f428-df63-4de8-b060-2d4038a5b1f4"
        );
    }
}
