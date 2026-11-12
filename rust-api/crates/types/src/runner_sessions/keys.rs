//! D-14 runner-session Redis keys + stream-id utils (pure, no I/O).
//!
//! Port of the key builders in `services/outbox.py:89-115`
//! (`stream_key`, `group_name`, `consumer_name`, `offline_stream_key`,
//! `session_eviction_channel`, `session_pel_drained_key`,
//! `stream_cleanup_zset_key`) and `services/machine_outbox.py:84-106`
//! (machine twins) plus `command_result_key` (`:339-341`), the legacy
//! Channels group name `pubsub.runner_group` (`pubsub.py:34-36`), and
//! the stream-id arithmetic in `outbox.py:687-737` (`_split_id`,
//! `_format_id`, `_min_stream_id`, `_decrement_stream_id`,
//! `id_for_secs_ago`).
//!
//! The `runner` / `machine` submodules mirror the two Python modules so
//! call sites keep the same short names. Fixture replayed by the unit
//! tests below: `rust-api/fixtures/runner_sessions/fx-rses-04-keys.json`
//! (FX-RSES-04).

use std::time::{SystemTime, UNIX_EPOCH};

/// Per-runner outbox keys (`services/outbox.py:89-115`).
pub mod runner {
    /// `runner_stream:{rid}` — persistent live stream (XADD targets).
    pub fn stream_key(runner_id: &str) -> String {
        format!("runner_stream:{runner_id}")
    }

    /// `runner-group:{rid}` — single consumer group on that stream.
    pub fn group_name(runner_id: &str) -> String {
        format!("runner-group:{runner_id}")
    }

    /// `consumer-{sid}` — one consumer name per session.
    pub fn consumer_name(session_id: &str) -> String {
        format!("consumer-{session_id}")
    }

    /// `runner_offline_stream:{rid}` — bounded offline buffer.
    pub fn offline_stream_key(runner_id: &str) -> String {
        format!("runner_offline_stream:{runner_id}")
    }

    /// `session_eviction:{rid}` — pub/sub channel for session evictions.
    pub fn session_eviction_channel(runner_id: &str) -> String {
        format!("session_eviction:{runner_id}")
    }

    /// `session_pel_drained:{sid}` — first-poll PEL-replay marker.
    pub fn session_pel_drained_key(session_id: &str) -> String {
        format!("session_pel_drained:{session_id}")
    }

    /// `runner_stream_cleanup` — sweeper zset (no id suffix).
    pub fn stream_cleanup_zset_key() -> &'static str {
        "runner_stream_cleanup"
    }

    /// Legacy Channels group name (`pubsub.py:34-36`), retained for the
    /// upgrade-ticket WS: `runner.{rid}` (dot, not colon).
    pub fn runner_group(runner_id: &str) -> String {
        format!("runner.{runner_id}")
    }
}

/// Per-dev-machine outbox keys (`services/machine_outbox.py:84-106,
/// :339-341`).
pub mod machine {
    /// `machine_stream:{mid}` — persistent live stream (XADD targets).
    pub fn stream_key(dev_machine_id: &str) -> String {
        format!("machine_stream:{dev_machine_id}")
    }

    /// `machine-group:{mid}` — single consumer group on that stream.
    pub fn group_name(dev_machine_id: &str) -> String {
        format!("machine-group:{dev_machine_id}")
    }

    /// `machine-consumer-{sid}` — one consumer name per session.
    pub fn consumer_name(session_id: &str) -> String {
        format!("machine-consumer-{session_id}")
    }

    /// `machine_offline_stream:{mid}` — bounded offline buffer.
    pub fn offline_stream_key(dev_machine_id: &str) -> String {
        format!("machine_offline_stream:{dev_machine_id}")
    }

    /// `machine_session_eviction:{mid}` — pub/sub channel for evictions.
    pub fn session_eviction_channel(dev_machine_id: &str) -> String {
        format!("machine_session_eviction:{dev_machine_id}")
    }

    /// `machine_session_pel_drained:{sid}` — first-poll PEL-replay marker.
    pub fn session_pel_drained_key(session_id: &str) -> String {
        format!("machine_session_pel_drained:{session_id}")
    }

    /// `machine_cmd_result:{request_id}` — short-TTL command-result key
    /// (`machine_outbox.py:339-341`).
    pub fn command_result_key(request_id: &str) -> String {
        format!("machine_cmd_result:{request_id}")
    }

    /// TTL for command-result keys (`machine_outbox.py:336`).
    pub const COMMAND_RESULT_TTL_SECS: u64 = 900;
}

/// Split a Redis stream id `"ms-seq"` into its parts
/// (`outbox.py:687-694`).
///
/// Returns `None` for a missing id, an id without `-`, or non-numeric
/// halves — the same inputs Python's `int()` rejects. Python's `int()`
/// also strips surrounding whitespace and accepts `+`/`-` signs (kept
/// here via `trim` + `i64` parsing); halves past `i64` range return
/// `None`, and more exotic spellings `int()` takes (underscores,
/// non-ASCII digits) never occur in Redis ids and are out of scope.
pub fn split_id(sid: Option<&str>) -> Option<(i64, i64)> {
    let sid = sid?;
    if sid.is_empty() || !sid.contains('-') {
        return None;
    }
    let (ms_str, seq_str) = sid.split_once('-')?;
    let ms: i64 = ms_str.trim().parse().ok()?;
    let seq: i64 = seq_str.trim().parse().ok()?;
    Some((ms, seq))
}

/// Format stream-id parts (`outbox.py:697-698`).
pub fn format_id(ms: i64, seq: i64) -> String {
    format!("{ms}-{seq}")
}

/// The smaller of two stream ids (`outbox.py:701-710`).
///
/// A missing side returns the other; a malformed side returns `None`
/// (the sweeper then skips the trim). The winner is re-rendered from
/// its parsed parts, exactly like Python's `_format_id(*pa/pb)`.
pub fn min_stream_id(a: Option<&str>, b: Option<&str>) -> Option<String> {
    match (a, b) {
        (None, None) => None,
        (None, other) | (other, None) => other.map(str::to_string),
        (Some(a), Some(b)) => {
            let pa = split_id(Some(a))?;
            let pb = split_id(Some(b))?;
            Some(if pa <= pb {
                format_id(pa.0, pa.1)
            } else {
                format_id(pb.0, pb.1)
            })
        }
    }
}

/// The largest stream id strictly less than `sid`
/// (`outbox.py:713-729`).
///
/// A zero sequence borrows from ms (`"…-0"` → `"(ms-1)-0"`, floored at
/// `0-0`); a malformed id passes through unchanged. The `(ms-1)-0`
/// borrow is verbatim from Python (its docstring notes it is only
/// `MINID`-safe, not the true predecessor).
pub fn decrement_stream_id(sid: &str) -> String {
    let Some((ms, seq)) = split_id(Some(sid)) else {
        return sid.to_string();
    };
    if seq > 0 {
        format_id(ms, seq - 1)
    } else {
        format_id(ms.saturating_sub(1).max(0), 0)
    }
}

/// A synthetic stream id whose ms portion is `now - secs * 1000`
/// (`outbox.py:732-737`), floored at `0-0`.
pub fn id_for_secs_ago(secs: i64) -> String {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let now_ms: i64 = now_ms.try_into().unwrap_or(i64::MAX);
    let ms = now_ms.saturating_sub(secs.saturating_mul(1000)).max(0);
    format_id(ms, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, Value};

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_sessions/fx-rses-04-keys.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn section<'a>(fx: &'a Value, key: &str) -> &'a Map<String, Value> {
        fx.get(key).and_then(Value::as_object).expect("section")
    }

    fn input_of<'a>(expected: &'a str, prefix: &str) -> &'a str {
        expected.strip_prefix(prefix).expect("prefix")
    }

    #[test]
    fn runner_keys_replay_fixture() {
        let fx = fixture();
        let keys = section(&fx, "runner_keys");
        let get = |field: &str| keys.get(field).and_then(Value::as_str).expect("key");
        assert_eq!(
            runner::stream_key(input_of(get("stream_key"), "runner_stream:")),
            get("stream_key")
        );
        assert_eq!(
            runner::group_name(input_of(get("group_name"), "runner-group:")),
            get("group_name")
        );
        assert_eq!(
            runner::consumer_name(input_of(get("consumer_name"), "consumer-")),
            get("consumer_name")
        );
        assert_eq!(
            runner::offline_stream_key(input_of(
                get("offline_stream_key"),
                "runner_offline_stream:"
            )),
            get("offline_stream_key")
        );
        assert_eq!(
            runner::session_eviction_channel(input_of(
                get("session_eviction_channel"),
                "session_eviction:"
            )),
            get("session_eviction_channel")
        );
        assert_eq!(
            runner::session_pel_drained_key(input_of(
                get("session_pel_drained_key"),
                "session_pel_drained:"
            )),
            get("session_pel_drained_key")
        );
        assert_eq!(
            runner::stream_cleanup_zset_key(),
            get("stream_cleanup_zset_key")
        );
        assert_eq!(
            runner::runner_group(input_of(get("runner_group_legacy_ws"), "runner.")),
            get("runner_group_legacy_ws")
        );
    }

    #[test]
    fn machine_keys_replay_fixture() {
        let fx = fixture();
        let keys = section(&fx, "machine_keys");
        let get = |field: &str| keys.get(field).and_then(Value::as_str).expect("key");
        assert_eq!(
            machine::stream_key(input_of(get("stream_key"), "machine_stream:")),
            get("stream_key")
        );
        assert_eq!(
            machine::group_name(input_of(get("group_name"), "machine-group:")),
            get("group_name")
        );
        assert_eq!(
            machine::consumer_name(input_of(get("consumer_name"), "machine-consumer-")),
            get("consumer_name")
        );
        assert_eq!(
            machine::offline_stream_key(input_of(
                get("offline_stream_key"),
                "machine_offline_stream:"
            )),
            get("offline_stream_key")
        );
        assert_eq!(
            machine::session_eviction_channel(input_of(
                get("session_eviction_channel"),
                "machine_session_eviction:"
            )),
            get("session_eviction_channel")
        );
        assert_eq!(
            machine::session_pel_drained_key(input_of(
                get("session_pel_drained_key"),
                "machine_session_pel_drained:"
            )),
            get("session_pel_drained_key")
        );
        assert_eq!(
            machine::command_result_key(input_of(get("command_result_key"), "machine_cmd_result:")),
            get("command_result_key")
        );
        // Read off `machine_outbox.py:336` (no fixture entry owns it).
        assert_eq!(machine::COMMAND_RESULT_TTL_SECS, 900);
    }

    /// `split_id` inputs: most fixture keys ARE the input; `empty` and
    /// `none_input` are labels for `""` and `None`.
    #[test]
    fn split_id_replays_fixture() {
        let fx = fixture();
        let utils = section(&fx, "stream_id_utils");
        let vectors = utils
            .get("_split_id")
            .and_then(Value::as_object)
            .expect("vectors");
        for (label, input) in [
            ("1714080000-0", Some("1714080000-0")),
            ("1714080000123-45", Some("1714080000123-45")),
            ("no-dash-here-x", Some("no-dash-here-x")),
            ("empty", Some("")),
            ("nodash", Some("nodash")),
            ("none_input", None),
        ] {
            let expected = vectors.get(label).expect("vector");
            let expected = match expected {
                Value::Null => None,
                Value::Array(pair) => Some((
                    pair[0].as_i64().expect("ms"),
                    pair[1].as_i64().expect("seq"),
                )),
                _ => panic!("vector shape"),
            };
            assert_eq!(split_id(input), expected, "label {label}");
        }
        // `non-numeric` is a label for an unrecorded non-numeric input;
        // the recorded output is null and any such input agrees.
        assert_eq!(vectors.get("non-numeric"), Some(&Value::Null));
        assert_eq!(split_id(Some("abc-def")), None);
        // Python-int leniency kept: surrounding whitespace and signs.
        assert_eq!(split_id(Some("  1714080000-0  ")), Some((1714080000, 0)));
        assert_eq!(split_id(Some("+5-+3")), Some((5, 3)));
        // Only the first dash splits; a second dash poisons the seq half.
        assert_eq!(split_id(Some("5-3-1")), None);
        assert_eq!(split_id(Some("5-")), None);
        assert_eq!(split_id(Some("-5")), None);
    }

    #[test]
    fn format_id_replays_fixture() {
        let fx = fixture();
        let utils = section(&fx, "stream_id_utils");
        let vectors = utils
            .get("_format_id")
            .and_then(Value::as_object)
            .expect("vectors");
        for ((ms, seq), label) in [
            ((1714080000, 0), "(1714080000, 0)"),
            ((1714080000123, 45), "(1714080000123, 45)"),
        ] {
            assert_eq!(
                Some(format_id(ms, seq).as_str()),
                vectors.get(label).and_then(Value::as_str),
                "label {label}"
            );
        }
    }

    /// `_min_stream_id` labels name the scenario; inputs are reconstructed
    /// from the labels and the golden outputs come from the fixture.
    #[test]
    fn min_stream_id_replays_fixture() {
        let fx = fixture();
        let utils = section(&fx, "stream_id_utils");
        let vectors = utils
            .get("_min_stream_id")
            .and_then(Value::as_object)
            .expect("vectors");
        for (label, a, b) in [
            ("(a<b)", Some("1714080001-0"), Some("1714080000-0")),
            ("(b<a)", Some("1714080000-0"), Some("1714080001-0")),
            ("(equal)", Some("1714080000-5"), Some("1714080000-5")),
            ("(seq-order)", Some("1714080000-9"), Some("1714080000-10")),
            ("(a-None)", None, Some("1714080000-0")),
            ("(b-None)", Some("1714080000-0"), None),
            ("(malformed)", Some("bogus"), Some("1714080000-0")),
        ] {
            let expected = match vectors.get(label).expect("vector") {
                Value::Null => None,
                Value::String(s) => Some(s.clone()),
                _ => panic!("vector shape"),
            };
            assert_eq!(min_stream_id(a, b), expected, "label {label}");
        }
        // Both missing: `if a is None: return b` → None (not in fixture).
        assert_eq!(min_stream_id(None, None), None);
    }

    /// `_decrement_stream_id` labels carry `(note)` suffixes except the
    /// malformed passthrough, whose input equals its output.
    #[test]
    fn decrement_stream_id_replays_fixture() {
        let fx = fixture();
        let utils = section(&fx, "stream_id_utils");
        let vectors = utils
            .get("_decrement_stream_id")
            .and_then(Value::as_object)
            .expect("vectors");
        for (label, input) in [
            ("1714080000-5", "1714080000-5"),
            ("1714080000-1", "1714080000-1"),
            ("1714080000-0(ms-0 borrow)", "1714080000-0"),
            ("0-0(floor)", "0-0"),
            ("malformed-passthrough", "bogus"),
        ] {
            assert_eq!(
                Some(decrement_stream_id(input).as_str()),
                vectors.get(label).and_then(Value::as_str),
                "label {label}"
            );
        }
    }

    #[test]
    fn id_for_secs_ago_window() {
        // The fixture value is time-bound, so replay the check, not the
        // value: well-formed id, zero seq, ms within the call window.
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_millis() as i64
            - 3600 * 1000;
        let id = id_for_secs_ago(3600);
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_millis() as i64
            - 3600 * 1000;
        let (ms, seq) = split_id(Some(&id)).expect("well-formed id");
        assert_eq!(seq, 0);
        assert!(
            (before..=after).contains(&ms),
            "id {id} outside [{before},{after}]"
        );
        assert!(id_for_secs_ago(0).ends_with("-0"));
    }
}
