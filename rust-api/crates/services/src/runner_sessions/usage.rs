#![forbid(unsafe_code)]

//! Canonical token-usage shape for the session service (D-14, stage 5).
//!
//! `upsert_runner_live_state` (`session_service.py:389`) stores the poll's
//! `tokens` object through `normalize_usage`
//! (`runner/services/usage.py:129-148`). That module is fully ported by
//! D-15 L1 ([`pidash_types::runner_runs::usage`], PIDASHCONV-534's
//! dependency chain, merged) — this module re-exports the L1 function
//! plus its public key tables so the upsert compiles against this
//! domain's path and `cloud_agent/runtime.py` reuses this port later,
//! exactly as PIDASHCONV-556 requires, without forking the logic.
//! (Reused, never redefined: the provider pick tables stay private in
//! L1; the D-14 matcher-drain precedent,
//! [`crate::runner_sessions::drain`], reuses L1/L2 the same way.)
//!
//! Fixture: `rust-api/fixtures/runner_sessions/fx-rses-06-session-service.json`
//! (FX-RSES-06 `normalize_usage`). The section carries outputs only, so
//! the tests below reconstruct the inputs: on the provider path `raw`
//! echoes the reported object verbatim (`usage.py:145`), hence the raw
//! value *is* the input; on the canonical path the input is the
//! counters plus their `raw` (`usage.py:140-142`). (Same recorded
//! shape as L1's merge vectors; see the L1 module docs.)
//!
//! Ported bugs: none found in this unit on read-through (L1's port
//! notes apply: the computed `total` skips the range check, `int()`
//! string quirks are ASCII-only).

pub use pidash_types::runner_runs::usage::{
    coerce_token, normalize_usage, BIGINT_MAX, CANONICAL_USAGE_KEYS,
};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, Value};

    /// FX-RSES-06 `normalize_usage` section.
    fn fx() -> Value {
        let text =
            include_str!("../../../../fixtures/runner_sessions/fx-rses-06-session-service.json");
        serde_json::from_str(text).expect("fixture parses")
    }

    /// `normalize_usage` over an object map (the poll `tokens` shape).
    fn norm(map: Map<String, Value>) -> Value {
        normalize_usage(&Value::Object(map))
    }

    #[test]
    fn empty_none_and_non_mapping_normalize_to_empty() {
        let fx = fx()["normalize_usage"].clone();
        assert_eq!(norm(Map::new()), fx["empty"]);
        assert_eq!(normalize_usage(&Value::Null), fx["none"]);
        assert_eq!(normalize_usage(&Value::from(7)), fx["non_mapping"]);
        assert_eq!(normalize_usage(&Value::from("tokens")), fx["non_mapping"]);
    }

    #[test]
    fn canonical_path_counters_plus_raw_replay() {
        let fx = fx()["normalize_usage"].clone();
        // Canonical input: the counters plus their verbatim `raw`.
        let mut input = Map::new();
        input.insert("input".to_string(), Value::from(10));
        input.insert("output".to_string(), Value::from(5));
        input.insert("total".to_string(), Value::from(15));
        let mut raw = Map::new();
        raw.insert("verbatim".to_string(), Value::Bool(true));
        input.insert("raw".to_string(), Value::Object(raw));
        assert_eq!(norm(input), fx["canonical"]);
        // Canonical sum: `total` computed when both legs present.
        let mut input = Map::new();
        input.insert("input".to_string(), Value::from(10));
        input.insert("output".to_string(), Value::from(5));
        assert_eq!(norm(input), fx["canonical_sum"]);
    }

    #[test]
    fn provider_paths_replay_with_raw_echo_inputs() {
        let fx = fx()["normalize_usage"].clone();
        // Provider input == the echoed `raw` object.
        for case in [
            "openai_chat",
            "anthropic",
            "codex_v2",
            "nested_reasoning",
            "nested_cache",
            "coerce_junk",
            "bigint_overflow",
        ] {
            let expected = fx[case].clone();
            let input = expected
                .get("raw")
                .cloned()
                .expect("provider case carries raw");
            assert_eq!(normalize_usage(&input), expected, "case {case}");
        }
    }

    #[test]
    fn key_tables_match_python_source() {
        // `usage.py:35-37`.
        assert_eq!(BIGINT_MAX, i64::MAX);
        assert_eq!(
            CANONICAL_USAGE_KEYS,
            [
                "input",
                "output",
                "total",
                "cache_read",
                "cache_write",
                "reasoning"
            ]
        );
        // `coerce_token` spot checks (`usage.py:69-79`): bools/empties
        // reject, negatives and >bigint reject, strings coerce.
        assert_eq!(coerce_token(&Value::Bool(true)), None);
        assert_eq!(coerce_token(&Value::from("")), None);
        assert_eq!(coerce_token(&Value::from(-3)), None);
        assert_eq!(coerce_token(&Value::from(i64::MAX)), Some(i64::MAX));
        assert_eq!(coerce_token(&Value::from("42")), Some(42));
    }
}
