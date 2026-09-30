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
//! Transport for the three behaviors above is the merged foundation
//! (`pidash_db::redis::RedisHandle`, PIDASHCONV-265): one shared client on
//! [`AppState`](crate::state::AppState) (`state.redis()`, `None` when the
//! cache is disabled), exposing `SET .. EX` / `GET` / `SUBSCRIBE`. This
//! module pins everything the transport executes — key formats, expiries,
//! and the throttle cache algorithm over the ported
//! [`crate::assistant::throttles`] pure functions — and owns the call sites'
//! failure policy, which mirrors Python per site (see each function).
//!
//! The SSE live tail's *receive* half is the merged
//! [`pidash_db::redis::RedisHandle::next_payload`] foundation method
//! (PIDASHCONV-267): awaiting the next publish needs a `Stream` poll that
//! neither this crate's dependency closure nor any new `assistant/` file
//! can name (`api/Cargo.toml` is read-only), so the poll lives in the db
//! crate and `events.rs` awaits plain bytes. When the tail cannot start
//! (no cache client, subscribe failure), `events.rs` serves the exact
//! replay prefix as a finite body — the bytes Python's `except` leaves
//! behind in those cases.

use std::time::{SystemTime, UNIX_EPOCH};

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
/// `threads.py:105-110`): [`cancel_set_command`] through the shared client.
/// Every failure is swallowed (`except Exception: pass`), including a
/// missing client (cache disabled): the 204 response shape never changes.
pub async fn signal_cancel(state: &AppState, turn_id: &Uuid) {
    let (key, value, expiry_secs) = cancel_set_command(turn_id);
    let Some(redis) = state.redis() else {
        return;
    };
    if let Err(error) = redis.set_ex(&key, &value, expiry_secs).await {
        tracing::debug!(%error, key = key.as_str(), "assistant.cancel: signal write failed; swallowed");
    }
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
/// (here [`MESSAGE_THROTTLE`]'s 3600s window). The pure decision behind
/// [`check_message_throttle`], kept so the algorithm stays unit-testable
/// without a Redis server.
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

/// Decode a cached throttle history: the JSON array of `time.time()` floats
/// this handler writes. Anything else (a cache miss is `None` before this;
/// notably a Django-pickled history from pre-cutover traffic, which is not
/// JSON) decodes to the empty history, i.e. fail-open to allow — the
/// foundation transport's documented policy for unreadable values.
pub fn decode_throttle_history(raw: &str) -> Vec<f64> {
    serde_json::from_str(raw).unwrap_or_default()
}

/// Encode a throttle history for the cache: the compact JSON array of
/// floats [`decode_throttle_history`] reads back.
pub fn encode_throttle_history(history: &[f64]) -> String {
    serde_json::to_string(history).expect("float vec serializes")
}

/// Check the message-POST brake for `user_id` (`messages.py:39-42`): the
/// DRF sliding window over the cached timestamp history at
/// [`message_throttle_key`]. `now` is `time.time()` seconds; entries `<=
/// now - duration` have passed out of the window. On allow the caller
/// records `now` at the front and re-caches with timeout `duration`
/// (`throttle_success`); on quota exhaustion the caller answers the
/// `Throttled` 429. A missing client, a cache miss, an unreadable value,
/// and a failed re-cache all fail open to allow (cutover traffic owns these
/// keys, so a miss is a fresh user, exactly DRF's `get(key, [])`).
pub async fn check_message_throttle(state: &AppState, user_id: &Uuid) -> ThrottleVerdict {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0);
    let key = message_throttle_key(user_id);
    let Some(redis) = state.redis() else {
        return ThrottleVerdict::Allow;
    };
    let history: Vec<f64> = match redis.get_string(&key).await {
        Ok(Some(raw)) => decode_throttle_history(&raw),
        _ => Vec::new(),
    };
    let window = MESSAGE_THROTTLE.window_secs as f64;
    let mut live: Vec<f64> = history
        .into_iter()
        .filter(|&stamp| stamp > now - window)
        .collect();
    if live.len() >= MESSAGE_THROTTLE.requests as usize {
        return ThrottleVerdict::Deny;
    }
    live.insert(0, now);
    let recached = encode_throttle_history(&live);
    if let Err(error) = redis
        .set_ex(&key, &recached, MESSAGE_THROTTLE.window_secs)
        .await
    {
        tracing::debug!(%error, key = key.as_str(), "assistant.throttle: re-cache failed; allowance stands");
    }
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
        let turn: Uuid = "db68f428-df63-4de8-b060-2d4038a5b1f4"
            .parse()
            .expect("uuid");
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
        let user: Uuid = "a7107351-38ac-4f3d-91d5-ee34c2459bd6"
            .parse()
            .expect("uuid");
        assert_eq!(
            message_throttle_key(&user),
            "throttle_assistant_message_a7107351-38ac-4f3d-91d5-ee34c2459bd6"
        );
    }

    #[test]
    fn throttle_trips_at_thirty_per_hour() {
        let now = 1_700_000_000.0;
        let history: Vec<f64> = (0..30).map(|i| now - f64::from(i) * 60.0).collect();
        assert_eq!(
            evaluate_message_throttle(&history, now),
            ThrottleVerdict::Deny
        );
        assert_eq!(
            evaluate_message_throttle(&history[1..], now),
            ThrottleVerdict::Allow
        );
    }

    #[test]
    fn throttle_history_codec_roundtrips_and_rejects_pickles() {
        let history = vec![1_700_000_000.123_456_7, 1_699_999_999.0, 42.0];
        let encoded = encode_throttle_history(&history);
        assert_eq!(decode_throttle_history(&encoded), history);
        // Integers decode as floats (DRF timer values are floats, but a
        // hand-written cache must not trip the brake either).
        assert_eq!(decode_throttle_history("[1,2]"), vec![1.0, 2.0]);
        // A Django-pickled history (pre-cutover traffic) is not JSON:
        // fail-open to the empty history, i.e. allow.
        assert!(decode_throttle_history("\u{80}\u{4}X\u{1e}\0\0\0").is_empty());
        assert!(decode_throttle_history("not json at all").is_empty());
    }

    #[test]
    fn live_tail_channel_matches_publish_channel() {
        let thread: Uuid = "db68f428-df63-4de8-b060-2d4038a5b1f4"
            .parse()
            .expect("uuid");
        assert_eq!(
            live_tail_channel(&thread),
            "assistant:thread:db68f428-df63-4de8-b060-2d4038a5b1f4"
        );
    }
}
