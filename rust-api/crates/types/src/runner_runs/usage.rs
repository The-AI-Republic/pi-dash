//! Canonical token-usage shape for agent runs (D-15, stage 5).
//!
//! Port of `apps/api/pi_dash/runner/services/usage.py:1-173`:
//!
//! * `BIGINT_MAX` (`:35`) → [`BIGINT_MAX`].
//! * `CANONICAL_USAGE_KEYS` (`:37`) → [`CANONICAL_USAGE_KEYS`].
//! * `coerce_token` (`:69-79`) → [`coerce_token`].
//! * `normalize_usage` (`:129-148`) → [`normalize_usage`].
//! * `merge_usage` (`:151-162`) → [`merge_usage`].
//! * `flat_token_fields` (`:165-173`) → [`flat_token_fields`].
//!
//! Translation notes:
//!
//! * `int(raw)` on strings accepts optional whitespace/sign plus digits
//!   with single underscores between digits (`int("1_0") == 10`,
//!   `int("1__0")` raises); the port parses exactly that, ASCII-only —
//!   Python also accepts Unicode decimal digits, which no provider emits.
//!   Floats truncate toward zero (`int(4.5) == 4`); NaN/inf coerce to
//!   `None` (unreachable from parsed JSON, which has no such literals).
//! * A computed `total` (`input + output`) is stored unchecked, exactly as
//!   Python skips the range check there — it can exceed [`BIGINT_MAX`].
//! * Output key order follows dict insertion order: canonical-order
//!   counters, computed `total` appended after them, `raw` last; merges
//!   keep first-insertion positions (`dict.update` semantics).
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-01-types-pure.golden.json`
//! (FX-RUN-01 `usage`). The `merge_usage` vectors carry case+output without
//! inputs (recorded that way; see the PIDASHCONV-549 review), so the merge
//! tests below use independent inputs whose outputs were verified against
//! the live Python functions.
//!
//! Ported bugs: none found in this unit on read-through.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Largest storable counter (`usage.py:35`): Postgres `bigint` max.
pub const BIGINT_MAX: i64 = 9223372036854775807;

/// Canonical counter keys (`usage.py:37`); any one of them present selects
/// the canonical path in [`normalize_usage`].
pub const CANONICAL_USAGE_KEYS: [&str; 6] = [
    "input",
    "output",
    "total",
    "cache_read",
    "cache_write",
    "reasoning",
];

const INPUT_KEYS: [&str; 3] = ["inputTokens", "input_tokens", "prompt_tokens"];
const OUTPUT_KEYS: [&str; 3] = ["outputTokens", "output_tokens", "completion_tokens"];
const TOTAL_KEYS: [&str; 2] = ["totalTokens", "total_tokens"];
const CACHE_READ_KEYS: [&str; 4] = [
    "cachedInputTokens",
    "cached_input_tokens",
    "cache_read_input_tokens",
    "cache_read_tokens",
];
const CACHE_READ_NESTED: [(&str, &str); 2] = [
    ("prompt_tokens_details", "cached_tokens"),
    ("input_tokens_details", "cached_tokens"),
];
const CACHE_WRITE_KEYS: [&str; 4] = [
    "cacheWriteInputTokens",
    "cache_write_input_tokens",
    "cache_creation_input_tokens",
    "cache_write_tokens",
];
const REASONING_KEYS: [&str; 2] = ["reasoningOutputTokens", "reasoning_output_tokens"];
const REASONING_NESTED: [(&str, &str); 3] = [
    ("completion_tokens_details", "reasoning_tokens"),
    ("output_tokens_details", "reasoning_tokens"),
    ("details", "reasoning_tokens"),
];
/// Anthropic reports cache reads/writes *beside* `input_tokens`; their
/// presence marks an Anthropic usage block (`usage.py:66`).
const ANTHROPIC_CACHE_KEYS: [&str; 2] = ["cache_read_input_tokens", "cache_creation_input_tokens"];

/// Python `str.strip()` for `int()`/counter parsing: Rust's `is_whitespace`
/// set plus U+001C..=U+001F (verified over the BMP: the only delta).
fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// `int(text)` for counter strings: optional ASCII whitespace (already
/// stripped), one optional sign, then digits with single underscores only
/// between digits. Returns the value for range-checking by the caller.
fn parse_int_str(text: &str) -> Option<i128> {
    let rest = py_strip(text);
    if rest.is_empty() {
        return None;
    }
    let (negative, digits) = match rest.strip_prefix('+') {
        Some(tail) => (false, tail),
        None => match rest.strip_prefix('-') {
            Some(tail) => (true, tail),
            None => (false, rest),
        },
    };
    if digits.is_empty() {
        return None;
    }
    let mut value: i128 = 0;
    let mut any_digit = false;
    let mut prev_underscore = false;
    for ch in digits.chars() {
        if ch == '_' {
            if !any_digit || prev_underscore {
                return None;
            }
            prev_underscore = true;
        } else if ch.is_ascii_digit() {
            any_digit = true;
            prev_underscore = false;
            value = value * 10 + (ch as i128 - '0' as i128);
            if value > BIGINT_MAX as i128 {
                // Still None however the tail parses (invalid, or valid
                // but out of range), so stop accumulating.
                return None;
            }
        } else {
            return None;
        }
    }
    if prev_underscore || !any_digit {
        return None;
    }
    Some(if negative { -value } else { value })
}

/// A non-negative bigint, or `None` for anything else (`usage.py:69-79`).
pub fn coerce_token(raw: &Value) -> Option<i64> {
    match raw {
        Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => None,
        Value::Number(n) => {
            if let Some(v) = n.as_i64() {
                (0..=BIGINT_MAX).contains(&v).then_some(v)
            } else if let Some(v) = n.as_u64() {
                (v <= BIGINT_MAX as u64).then_some(v as i64)
            } else if let Some(f) = n.as_f64() {
                // Truncation toward zero; `2^63` (or more, or non-finite)
                // can never truncate into range — the `as f64` of
                // BIGINT_MAX itself rounds up to 2^63, so compare against
                // 2^63 exactly.
                if !f.is_finite() || !(0.0..9_223_372_036_854_775_808.0).contains(&f) {
                    None
                } else {
                    Some(f.trunc() as i64)
                }
            } else {
                None
            }
        }
        Value::String(s) => {
            let value = parse_int_str(s)?;
            (0..=BIGINT_MAX as i128)
                .contains(&value)
                .then_some(value as i64)
        }
    }
}

/// First coercible counter under `keys` (`usage.py:82-87`).
fn pick(usage: &Map<String, Value>, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|k| coerce_token(usage.get(*k).unwrap_or(&Value::Null)))
}

/// First coercible counter under a nested `(outer, inner)` path
/// (`usage.py:90-97`); the outer value must be a mapping.
fn pick_nested(usage: &Map<String, Value>, paths: &[(&str, &str)]) -> Option<i64> {
    for (outer, inner) in paths {
        if let Some(Value::Object(container)) = usage.get(*outer) {
            let value = coerce_token(container.get(*inner).unwrap_or(&Value::Null));
            if value.is_some() {
                return value;
            }
        }
    }
    None
}

/// First non-`None` counter (`usage.py:100-101`).
fn first(values: &[Option<i64>]) -> Option<i64> {
    values.iter().copied().flatten().next()
}

/// Store a computed counter without the [`coerce_token`] range check
/// (`usage.py:104-107`, `:124-125`): Python keeps `input + output` and the
/// Anthropic-adjusted `input` unchecked.
fn store_computed(value: i128) -> Value {
    if value <= BIGINT_MAX as i128 {
        Value::from(value as i64)
    } else if value <= u64::MAX as i128 {
        Value::from(value as u64)
    } else {
        // Absurd inputs only (each source counter is <= BIGINT_MAX, so
        // this needs input + both caches past u64::MAX); saturate, since
        // JSON has no bigger integer literal.
        Value::from(u64::MAX)
    }
}

/// Read back a counter this module stored (an `i64`, or a `u64` when a
/// computed value passed `i64::MAX` unchecked, as Python's does).
fn stored_counter(counters: &Map<String, Value>, key: &str) -> Option<i128> {
    match counters.get(key) {
        Some(Value::Number(n)) => n
            .as_i64()
            .map(|v| v as i128)
            .or_else(|| n.as_u64().map(|v| v as i128)),
        _ => None,
    }
}

/// `total = input + output` when `total` is absent and both are present
/// (`usage.py:104-107`); appended after the existing counters.
fn with_total(mut counters: Map<String, Value>) -> Map<String, Value> {
    if !counters.contains_key("total") {
        if let (Some(a), Some(b)) = (
            stored_counter(&counters, "input"),
            stored_counter(&counters, "output"),
        ) {
            counters.insert("total".to_string(), store_computed(a + b));
        }
    }
    counters
}

/// Canonical-path counters (`usage.py:110-112`), in
/// [`CANONICAL_USAGE_KEYS`] order.
fn canonical_counters(usage: &Map<String, Value>) -> Map<String, Value> {
    let mut counters = Map::new();
    for key in CANONICAL_USAGE_KEYS {
        if let Some(value) = coerce_token(usage.get(key).unwrap_or(&Value::Null)) {
            counters.insert(key.to_string(), Value::from(value));
        }
    }
    with_total(counters)
}

/// Provider-path counters (`usage.py:115-126`).
fn provider_counters(usage: &Map<String, Value>) -> Map<String, Value> {
    let input = pick(usage, &INPUT_KEYS);
    let output = pick(usage, &OUTPUT_KEYS);
    let total = pick(usage, &TOTAL_KEYS);
    let cache_read = first(&[
        pick(usage, &CACHE_READ_KEYS),
        pick_nested(usage, &CACHE_READ_NESTED),
    ]);
    let cache_write = pick(usage, &CACHE_WRITE_KEYS);
    let reasoning = first(&[
        pick(usage, &REASONING_KEYS),
        pick_nested(usage, &REASONING_NESTED),
    ]);
    let mut counters = Map::new();
    if let Some(v) = input {
        if ANTHROPIC_CACHE_KEYS.iter().any(|k| usage.contains_key(*k)) {
            let adjusted =
                v as i128 + cache_read.unwrap_or(0) as i128 + cache_write.unwrap_or(0) as i128;
            counters.insert("input".to_string(), store_computed(adjusted));
        } else {
            counters.insert("input".to_string(), Value::from(v));
        }
    }
    for (key, value) in [
        ("output", output),
        ("total", total),
        ("cache_read", cache_read),
        ("cache_write", cache_write),
        ("reasoning", reasoning),
    ] {
        if let Some(v) = value {
            counters.insert(key.to_string(), Value::from(v));
        }
    }
    with_total(counters)
}

/// `raw` passes through verbatim unless it is `None`/`{}`/`""`
/// (`usage.py:146-147`); every other value — `0`, `false`, `[]` — is kept.
fn keep_raw(raw: Option<&Value>) -> bool {
    match raw {
        None | Some(Value::Null) => false,
        Some(Value::Object(m)) if m.is_empty() => false,
        Some(Value::String(s)) if s.is_empty() => false,
        _ => true,
    }
}

/// Map a reported usage object onto the canonical shape
/// (`usage.py:129-148`). Returns `{}` when nothing usable was reported.
pub fn normalize_usage(reported: &Value) -> Value {
    let Some(map) = reported.as_object() else {
        return Value::Object(Map::new());
    };
    if map.is_empty() {
        return Value::Object(Map::new());
    }
    let usage = if CANONICAL_USAGE_KEYS.iter().any(|k| map.contains_key(*k)) {
        let mut usage = canonical_counters(map);
        if keep_raw(map.get("raw")) {
            usage.insert(
                "raw".to_string(),
                map.get("raw").cloned().unwrap_or(Value::Null),
            );
        }
        usage
    } else {
        let mut usage = provider_counters(map);
        // The provider object is non-empty here, so `raw` is always kept.
        usage.insert("raw".to_string(), Value::Object(map.clone()));
        usage
    };
    Value::Object(usage)
}

/// Normalise each source and overlay them, later sources winning per key
/// (`usage.py:151-162`). A counter the fresher source did not report keeps
/// the older value; there is no cross-source `total` recompute.
pub fn merge_usage(sources: &[Value]) -> Value {
    let mut merged = Map::new();
    for source in sources {
        if let Value::Object(map) = normalize_usage(source) {
            merged.extend(map);
        }
    }
    Value::Object(merged)
}

/// The legacy `input_tokens` / `output_tokens` / `total_tokens` view of a
/// usage object (`usage.py:165-173`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatTokenFields {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}

/// Read the flat legacy token fields off a usage object.
pub fn flat_token_fields(usage: &Value) -> FlatTokenFields {
    let get = |key: &str| -> Option<i64> {
        match usage.as_object().and_then(|m| m.get(key)) {
            Some(v) => coerce_token(v),
            None => None,
        }
    };
    FlatTokenFields {
        input_tokens: get("input"),
        output_tokens: get("output"),
        total_tokens: get("total"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-01-types-pure.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn usage_section(fx: &Value) -> &Map<String, Value> {
        fx.get("usage")
            .and_then(Value::as_object)
            .expect("usage section")
    }

    fn keys_of(value: &Value) -> Vec<String> {
        value
            .as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    #[test]
    fn bigint_max_and_canonical_keys_match_fixture() {
        let fx = fixture();
        let meta = fx.get("enum_meta").expect("enum_meta");
        assert_eq!(
            meta.get("BIGINT_MAX").and_then(Value::as_i64),
            Some(BIGINT_MAX)
        );
        let keys: Vec<String> = CANONICAL_USAGE_KEYS.iter().map(|s| s.to_string()).collect();
        assert_eq!(
            meta.get("CANONICAL_USAGE_KEYS"),
            Some(&Value::Array(keys.into_iter().map(Value::String).collect()))
        );
        assert_eq!(BIGINT_MAX, i64::MAX);
    }

    /// The fixture records coerce inputs as Python reprs; map each to JSON.
    fn coerce_input(repr: &str) -> Value {
        match repr {
            "None" => Value::Null,
            "''" => json!(""),
            "True" => json!(true),
            "False" => json!(false),
            "0" => json!(0),
            "5" => json!(5),
            "-1" => json!(-1),
            "'42'" => json!("42"),
            "' 7 '" => json!(" 7 "),
            "'4.5'" => json!("4.5"),
            "4.5" => json!(4.5),
            "4.0" => json!(4.0),
            "'0x10'" => json!("0x10"),
            "9223372036854775807" => json!(9223372036854775807i64),
            "9223372036854775808" => json!(9223372036854775808u64),
            "'9223372036854775807'" => json!("9223372036854775807"),
            "'9223372036854775808'" => json!("9223372036854775808"),
            "[]" => json!([]),
            "{}" => json!({}),
            "'abc'" => json!("abc"),
            "'12abc'" => json!("12abc"),
            other => panic!("unmapped coerce repr {other}"),
        }
    }

    #[test]
    fn coerce_token_replays_fixture_vectors() {
        let fx = fixture();
        let vectors = usage_section(&fx)
            .get("coerce_token")
            .and_then(Value::as_array)
            .expect("coerce vectors");
        assert!(!vectors.is_empty());
        for v in vectors {
            let input = v.get("in").and_then(Value::as_str).expect("in repr");
            let expected = v.get("out").expect("out");
            let actual = coerce_token(&coerce_input(input));
            assert_eq!(
                &actual.map(Value::from).unwrap_or(Value::Null),
                expected,
                "coerce_token({input})"
            );
        }
    }

    #[test]
    fn coerce_token_matches_python_int_edges() {
        // Verified against CPython `int()` + the live `coerce_token`.
        let cases: &[(&str, Option<i64>)] = &[
            ("1_0", Some(10)),
            ("+42", Some(42)),
            ("-0", Some(0)),
            ("  -3  ", None), // -3 < 0
            ("0_0", Some(0)),
            ("1__0", None),
            ("_1", None),
            ("1_", None),
            ("+ 1", None),
            ("4_2.5", None),
            ("", None),
            ("   ", None),
            ("+", None),
            ("-", None),
            ("007", Some(7)),
        ];
        for (text, expected) in cases {
            assert_eq!(
                coerce_token(&json!(text)),
                *expected,
                "coerce_token({text:?})"
            );
        }
        assert_eq!(coerce_token(&json!(-4.5)), None); // truncates to -4, then < 0
        assert_eq!(coerce_token(&json!(9.99)), Some(9));
        assert_eq!(coerce_token(&json!(0.0)), Some(0));
        assert_eq!(coerce_token(&json!(1e19)), None); // past BIGINT_MAX
        assert_eq!(coerce_token(&json!(-0.0)), Some(0));
    }

    #[test]
    fn normalize_usage_replays_fixture_vectors_in_order() {
        let fx = fixture();
        let vectors = usage_section(&fx)
            .get("normalize_usage")
            .and_then(Value::as_array)
            .expect("normalize vectors");
        assert_eq!(vectors.len(), 24);
        for v in vectors {
            let case = v.get("case").and_then(Value::as_str).unwrap_or("?");
            let input = v.get("in").unwrap_or(&Value::Null);
            let expected = v.get("out").expect("out");
            let actual = normalize_usage(input);
            assert_eq!(&actual, expected, "normalize_usage({case})");
            assert_eq!(
                keys_of(&actual),
                keys_of(expected),
                "normalize_usage({case}) key order"
            );
        }
    }

    #[test]
    fn merge_usage_freshest_wins_per_counter_without_total_recompute() {
        // Independent inputs (the fixture carries outputs only); every
        // expectation below was verified against the live Python
        // `merge_usage` before encoding.
        assert_eq!(merge_usage(&[]), json!({}));
        assert_eq!(
            merge_usage(&[json!({"input": 1, "output": 2})]),
            json!({"input": 1, "output": 2, "total": 3})
        );
        // Later source wins per key; its `raw` replaces the older one.
        assert_eq!(
            merge_usage(&[
                json!({"input": 100, "output": 50}),
                json!({"input_tokens": 120, "output_tokens": 60})
            ]),
            json!({"input": 120, "output": 60, "total": 180, "raw": {"input_tokens": 120, "output_tokens": 60}}),
        );
        // A counter the fresher source omits survives; totals never
        // recompute across sources (first source's 150 stands over 170).
        assert_eq!(
            merge_usage(&[json!({"input": 100, "output": 50}), json!({"input": 120})]),
            json!({"input": 120, "output": 50, "total": 150}),
        );
        // New keys append; overwritten keys keep their position.
        assert_eq!(
            merge_usage(&[
                json!({"input": 100, "cache_read": 90}),
                json!({"output": 60})
            ]),
            json!({"input": 100, "cache_read": 90, "output": 60}),
        );
        assert_eq!(
            merge_usage(&[
                json!({"input": 1}),
                json!({"output": 2}),
                json!({"input": 3, "total": 99})
            ]),
            json!({"input": 3, "output": 2, "total": 99}),
        );
        let merged = merge_usage(&[
            json!({"input": 1}),
            json!({"output": 2}),
            json!({"input": 3, "total": 99}),
        ]);
        assert_eq!(keys_of(&merged), ["input", "output", "total"]);
        // `None`/unusable sources normalize to {} and change nothing.
        assert_eq!(
            merge_usage(&[Value::Null, json!({"input": 1, "output": 5, "total": 3})]),
            json!({"input": 1, "output": 5, "total": 3}),
        );
    }

    #[test]
    fn flat_token_fields_replays_fixture_vectors() {
        let fx = fixture();
        let vectors = usage_section(&fx)
            .get("flat_token_fields")
            .and_then(Value::as_array)
            .expect("flat vectors");
        assert!(!vectors.is_empty());
        for v in vectors {
            let input = v.get("in").unwrap_or(&Value::Null);
            let expected = v.get("out").expect("out");
            let actual = serde_json::to_value(flat_token_fields(input)).expect("serializes");
            assert_eq!(&actual, expected, "flat_token_fields({input})");
            assert_eq!(
                keys_of(&actual),
                ["input_tokens", "output_tokens", "total_tokens"]
            );
        }
    }
}
