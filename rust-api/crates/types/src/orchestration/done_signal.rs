//! Parser for the agent's terminal ``pi-dash-done`` fenced block (D-12, stage 5).
//!
//! Port of the parse half of `apps/api/pi_dash/orchestration/done_signal.py`:
//!
//! * `FENCE_RE` (`:27-30`) → [`extract_fence`] (same pattern).
//! * `VALID_STATUSES` (`:32`) → [`VALID_STATUSES`].
//! * `DoneSignalError` (`:35-36`) → [`DoneSignalError`].
//! * `DoneSignal` (`:39-42`) → [`DoneSignal`].
//! * `extract_fence` (`:45-53`) → [`extract_fence`].
//! * `parse` (`:56-87`) → [`parse`].
//! * `_normalize` (`:90-138`) → [`normalize`].
//!
//! (`ingest_into_run` (`:141-181`) is L2's, in `pidash_db::orchestration`.)
//!
//! Translation notes:
//!
//! * The fence pattern uses no lookaround or backreferences, so the `regex`
//!   crate matches Python `re` exactly here (leftmost-first, `(?m)` line
//!   anchors, lazy body). Last fence wins, as in Python.
//! * `body.strip()` is `strip_python_whitespace` (see `mod.rs`): Rust
//!   `trim` misses U+001C..=U+001F, which Python strips.
//! * JSON errors surface only `exc.msg`, so the port needs CPython's
//!   message taxonomy, not its positions: valid bodies parse through
//!   `serde_json`, and failures are classified by a small first-error
//!   walker (`classify_json_error`) covering the nine `JSONDecodeError`
//!   messages. Inputs CPython accepts but `serde_json` rejects (bare
//!   `NaN`/`Infinity`, lone surrogates, >128-deep nesting) have no
//!   CPython message: top-level scalars flow into the same "must be a
//!   JSON object" error CPython reaches, while gap objects/arrays
//!   surface `serde_json`'s diagnostic. Integers past the `u64` range
//!   likewise parse as `f64`, so a huge `status` renders in exponent
//!   form where CPython keeps exact digits (error strings only).
//! * `got {status!r}` is `py_repr` (below): `None`/`True`/`False`,
//!   shortest floats in CPython exponent spelling, Python string quoting.
//!   Only ASCII controls are escaped; other non-ASCII scalars pass through
//!   (Python would escape unprintables — unreachable in real statuses).
//! * Truthy-but-not-a-dict sections (e.g. `"autonomy": [1]`) crash
//!   Python with `AttributeError`; the port totalizes them to defaults.
//!
//! Fixtures: `rust-api/fixtures/orchestration/fx01_types/done_signal.*.golden.json`
//! (FX-ORCH-01).
//!
//! Ported bugs: none found in this unit on read-through.

use serde_json::{Map, Value};
use std::sync::OnceLock;

/// Done-signal statuses (`done_signal.py:32`), in `sorted()` order — the
/// order the status error string renders them in.
pub const VALID_STATUSES: &[&str] = &["blocked", "completed", "noop", "paused"];

/// Raised when the done signal is missing or malformed
/// (`done_signal.py:35-36`). The message is the whole error, verbatim.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct DoneSignalError(pub String);

/// A parsed done signal: the status plus the normalized payload
/// (`done_signal.py:39-42`).
#[derive(Debug, Clone, PartialEq)]
pub struct DoneSignal {
    pub status: String,
    pub payload: Value,
}

fn fence_re() -> &'static regex::Regex {
    static FENCE: OnceLock<regex::Regex> = OnceLock::new();
    FENCE.get_or_init(|| {
        regex::Regex::new(r"(?ms)^```pi-dash-done[ \t]*\n(?P<body>.*?)^```[ \t]*$")
            .expect("fence pattern compiles")
    })
}

/// Extract the fenced JSON body, preferring the last fence
/// (`done_signal.py:45-53`).
///
/// `None` (and `""`) model the falsy inputs Python's `if not text` rejects.
pub fn extract_fence(text: Option<&str>) -> Option<String> {
    let text = text?;
    if text.is_empty() {
        return None;
    }
    let body = fence_re().captures_iter(text).last()?["body"].to_string();
    Some(super::strip_python_whitespace(&body).to_string())
}

/// Parse the terminal-turn output into a normalized payload
/// (`done_signal.py:56-87`).
pub fn parse(text: &str) -> Result<DoneSignal, DoneSignalError> {
    let body = extract_fence(Some(text))
        .ok_or_else(|| DoneSignalError("no pi-dash-done fenced block found".to_string()))?;
    let payload = parse_json_cpython(&body)
        .map_err(|msg| DoneSignalError(format!("pi-dash-done JSON invalid: {msg}")))?;
    let obj = payload
        .as_object()
        .ok_or_else(|| DoneSignalError("pi-dash-done payload must be a JSON object".to_string()))?;

    let status = obj.get("status").unwrap_or(&Value::Null);
    let status_str = status
        .as_str()
        .filter(|s| VALID_STATUSES.contains(s))
        .ok_or_else(|| {
            let want = VALID_STATUSES
                .iter()
                .map(|s| py_repr_string(s))
                .collect::<Vec<_>>()
                .join(", ");
            DoneSignalError(format!(
                "pi-dash-done.status must be one of [{want}]; got {}",
                py_repr(status)
            ))
        })?;

    if status_str == "paused" {
        let question = obj
            .get("autonomy")
            .and_then(Value::as_object)
            .and_then(|autonomy| autonomy.get("question_for_human"));
        let asked = question.is_some_and(super::is_truthy);
        if !asked {
            return Err(DoneSignalError(
                "pi-dash-done.status='paused' requires autonomy.question_for_human to be set"
                    .to_string(),
            ));
        }
    }

    Ok(DoneSignal {
        status: status_str.to_string(),
        payload: normalize(obj),
    })
}

/// Fill in defaults for optional fields (`done_signal.py:90-138`).
///
/// Key order, defaults, and the `or []` / `or {}` falsy rules are verbatim;
/// unknown keys are dropped. `status` is read from the payload (present in
/// the `parse` flow; `Null` when called directly without one).
pub fn normalize(payload: &Map<String, Value>) -> Value {
    let section = |key: &str| payload.get(key).and_then(Value::as_object);
    let get = |key: &str, section: Option<&Map<String, Value>>, default: Value| -> Value {
        section.and_then(|m| m.get(key)).cloned().unwrap_or(default)
    };
    let get_or_list = |key: &str, section: Option<&Map<String, Value>>| -> Value {
        let value = section.and_then(|m| m.get(key)).unwrap_or(&Value::Null);
        if super::is_truthy(value) {
            value.clone()
        } else {
            Value::Array(vec![])
        }
    };

    let autonomy = section("autonomy");
    let state_transition = section("state_transition");
    let changes = section("changes");
    let validation = section("validation");
    let progress = section("progress");

    let checkpoints = {
        let value = progress
            .and_then(|m| m.get("checkpoints"))
            .unwrap_or(&Value::Null);
        if super::is_truthy(value) {
            value.clone()
        } else {
            Value::Object(Map::new())
        }
    };
    let blockers = {
        let value = payload.get("blockers").unwrap_or(&Value::Null);
        if super::is_truthy(value) {
            value.clone()
        } else {
            Value::Array(vec![])
        }
    };

    serde_json::json!({
        "status": payload.get("status").cloned().unwrap_or(Value::Null),
        "summary": payload.get("summary").cloned().unwrap_or(Value::String(String::new())),
        "state_transition": {
            "requested_group": get("requested_group", state_transition, Value::Null),
            "reason": get("reason", state_transition, Value::Null),
        },
        "changes": {
            "branch": get("branch", changes, Value::Null),
            "commits": get_or_list("commits", changes),
            "files_touched": get_or_list("files_touched", changes),
            "pr_url": get("pr_url", changes, Value::Null),
        },
        "validation": {
            "acceptance_all_met": get("acceptance_all_met", validation, Value::Null),
            "ran": get_or_list("ran", validation),
            "notes": get("notes", validation, Value::Null),
        },
        "progress": {
            "phase": get("phase", progress, Value::Null),
            "checkpoints": checkpoints,
        },
        "autonomy": {
            "score": get("score", autonomy, Value::from(0)),
            "type": get("type", autonomy, Value::String("none".to_string())),
            "reason": get("reason", autonomy, Value::String(String::new())),
            "question_for_human": get("question_for_human", autonomy, Value::Null),
            "safe_to_continue": get("safe_to_continue", autonomy, Value::Bool(true)),
        },
        "blockers": blockers,
    })
}

/// Parse a fence body, reporting failures with CPython's `JSONDecodeError`
/// message.
fn parse_json_cpython(body: &str) -> Result<Value, String> {
    match serde_json::from_str(body) {
        Ok(value) => Ok(value),
        Err(serde_err) => match classify_json_error(body) {
            JsonError::Invalid(msg) => Err(msg),
            // A valid non-object scalar (bare `NaN`, a lone-surrogate
            // string): any non-object value flows into the same
            // "must be a JSON object" error CPython reaches.
            JsonError::GapScalar => Ok(Value::Null),
            JsonError::GapValue => Err(serde_err.to_string()),
        },
    }
}

/// Maximum nesting the error classifier descends before giving up (the
/// input already failed `serde_json`; past this budget its diagnostic is
/// surfaced instead of a CPython message).
const CLASSIFY_MAX_DEPTH: usize = 1024;

/// Outcome of classifying a body `serde_json` rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
enum JsonError {
    /// A grammar failure with CPython's `JSONDecodeError.msg`.
    Invalid(String),
    /// Grammar-valid top-level scalar (the `serde_json` ⊂ CPython gap):
    /// CPython parses it, then rejects it as a non-object.
    GapScalar,
    /// Grammar-valid object/array, or past the depth budget: no CPython
    /// message exists, so the caller surfaces `serde_json`'s diagnostic.
    GapValue,
}

/// Classify a JSON syntax failure into CPython's `JSONDecodeError.msg`.
fn classify_json_error(body: &str) -> JsonError {
    let mut walker = JsonWalker {
        bytes: body.as_bytes(),
        pos: 0,
        depth: 0,
        top_scalar: false,
    };
    match walker.parse_value() {
        Err(WalkError::Budget) => JsonError::GapValue,
        Err(WalkError::Msg(msg)) => JsonError::Invalid(msg.to_string()),
        Ok(()) => {
            walker.skip_ws();
            if walker.pos == walker.bytes.len() {
                if walker.top_scalar {
                    JsonError::GapScalar
                } else {
                    JsonError::GapValue
                }
            } else {
                JsonError::Invalid("Extra data".to_string())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WalkError {
    Msg(&'static str),
    Budget,
}

struct JsonWalker<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
    top_scalar: bool,
}

impl JsonWalker<'_> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len()
            && matches!(self.bytes[self.pos], b' ' | b'\t' | b'\n' | b'\r')
        {
            self.pos += 1;
        }
    }

    fn parse_value(&mut self) -> Result<(), WalkError> {
        self.skip_ws();
        if self.pos >= self.bytes.len() {
            return Err(WalkError::Msg("Expecting value"));
        }
        if self.depth == 0 && !matches!(self.bytes[self.pos], b'{' | b'[') {
            self.top_scalar = true;
        }
        match self.bytes[self.pos] {
            b'{' => {
                self.pos += 1;
                self.parse_object()
            }
            b'[' => {
                self.pos += 1;
                self.parse_array()
            }
            b'"' => self.parse_string(),
            b't' => self.expect_literal("true"),
            b'f' => self.expect_literal("false"),
            b'n' => self.expect_literal("null"),
            // `json.loads` accepts the constants (no `parse_constant` override).
            b'N' => self.expect_literal("NaN"),
            b'I' => self.expect_literal("Infinity"),
            b'-' => {
                if self.bytes[self.pos..].starts_with(b"-Infinity") {
                    self.expect_literal("-Infinity")
                } else {
                    self.parse_number()
                }
            }
            b'0'..=b'9' => self.parse_number(),
            _ => Err(WalkError::Msg("Expecting value")),
        }
    }

    fn expect_literal(&mut self, literal: &str) -> Result<(), WalkError> {
        if self.bytes[self.pos..].starts_with(literal.as_bytes()) {
            self.pos += literal.len();
            Ok(())
        } else {
            Err(WalkError::Msg("Expecting value"))
        }
    }

    fn parse_number(&mut self) -> Result<(), WalkError> {
        let start = self.pos;
        if self.bytes.get(self.pos) == Some(&b'-') {
            self.pos += 1;
        }
        match self.bytes.get(self.pos) {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                while matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => {
                self.pos = start;
                return Err(WalkError::Msg("Expecting value"));
            }
        }
        // A fraction/exponent is only consumed when fully valid; otherwise
        // the caller reports the delimiter error, as CPython's NUMBER_RE
        // backtracking does.
        if self.bytes.get(self.pos) == Some(&b'.')
            && matches!(self.bytes.get(self.pos + 1), Some(b'0'..=b'9'))
        {
            self.pos += 2;
            while matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.bytes.get(self.pos), Some(b'e' | b'E')) {
            let mut end = self.pos + 1;
            if matches!(self.bytes.get(end), Some(b'+' | b'-')) {
                end += 1;
            }
            if matches!(self.bytes.get(end), Some(b'0'..=b'9')) {
                end += 1;
                while matches!(self.bytes.get(end), Some(b'0'..=b'9')) {
                    end += 1;
                }
                self.pos = end;
            }
        }
        Ok(())
    }

    fn parse_string(&mut self) -> Result<(), WalkError> {
        debug_assert_eq!(self.bytes[self.pos], b'"');
        self.pos += 1;
        loop {
            if self.pos >= self.bytes.len() {
                return Err(WalkError::Msg("Unterminated string starting at"));
            }
            match self.bytes[self.pos] {
                b'"' => {
                    self.pos += 1;
                    return Ok(());
                }
                b'\\' => {
                    self.pos += 1;
                    if self.pos >= self.bytes.len() {
                        return Err(WalkError::Msg("Unterminated string starting at"));
                    }
                    match self.bytes[self.pos] {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {
                            self.pos += 1;
                        }
                        b'u' => {
                            let digits = self.bytes.get(self.pos + 1..self.pos + 5);
                            let valid = digits.is_some_and(|d| {
                                d.len() == 4 && d.iter().all(u8::is_ascii_hexdigit)
                            });
                            if !valid {
                                return Err(WalkError::Msg("Invalid \\uXXXX escape"));
                            }
                            self.pos += 5;
                        }
                        _ => return Err(WalkError::Msg("Invalid \\escape")),
                    }
                }
                0x00..=0x1f => return Err(WalkError::Msg("Invalid control character at")),
                _ => self.pos += 1,
            }
        }
    }

    fn enter_container(&mut self) -> Result<(), WalkError> {
        // Depth budget: the top level converts exhaustion to the gap
        // fallback. Deep-enough nesting already failed `serde_json`,
        // so its diagnostic fits.
        if self.depth >= CLASSIFY_MAX_DEPTH {
            return Err(WalkError::Budget);
        }
        self.depth += 1;
        Ok(())
    }

    fn parse_object(&mut self) -> Result<(), WalkError> {
        self.enter_container()?;
        self.skip_ws();
        if self.pos >= self.bytes.len() {
            return Err(WalkError::Msg(
                "Expecting property name enclosed in double quotes",
            ));
        }
        if self.bytes[self.pos] == b'}' {
            self.pos += 1;
            self.depth -= 1;
            return Ok(());
        }
        loop {
            self.skip_ws();
            if self.pos >= self.bytes.len() || self.bytes[self.pos] != b'"' {
                return Err(WalkError::Msg(
                    "Expecting property name enclosed in double quotes",
                ));
            }
            self.parse_string()?;
            self.skip_ws();
            if self.pos >= self.bytes.len() || self.bytes[self.pos] != b':' {
                return Err(WalkError::Msg("Expecting ':' delimiter"));
            }
            self.pos += 1;
            self.parse_value()?;
            self.skip_ws();
            if self.pos >= self.bytes.len() {
                return Err(WalkError::Msg("Expecting ',' delimiter"));
            }
            match self.bytes[self.pos] {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Ok(());
                }
                _ => return Err(WalkError::Msg("Expecting ',' delimiter")),
            }
        }
    }

    fn parse_array(&mut self) -> Result<(), WalkError> {
        self.enter_container()?;
        self.skip_ws();
        if self.pos >= self.bytes.len() {
            return Err(WalkError::Msg("Expecting value"));
        }
        if self.bytes[self.pos] == b']' {
            self.pos += 1;
            self.depth -= 1;
            return Ok(());
        }
        loop {
            self.parse_value()?;
            self.skip_ws();
            if self.pos >= self.bytes.len() {
                return Err(WalkError::Msg("Expecting ',' delimiter"));
            }
            match self.bytes[self.pos] {
                b',' => self.pos += 1,
                b']' => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Ok(());
                }
                _ => return Err(WalkError::Msg("Expecting ',' delimiter")),
            }
        }
    }
}

/// Python `repr()` for a JSON value, as `got {status!r}` renders it.
fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else {
                cpython_float_repr(n.as_f64().unwrap_or(f64::NAN))
            }
        }
        Value::String(s) => py_repr_string(s),
        Value::Array(items) => {
            let inner = items.iter().map(py_repr).collect::<Vec<_>>().join(", ");
            format!("[{inner}]")
        }
        Value::Object(map) => {
            let inner = map
                .iter()
                .map(|(k, v)| format!("{}: {}", py_repr_string(k), py_repr(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{inner}}}")
        }
    }
}

/// Python `repr()` for a string: single quotes unless the text holds a
/// single (and no double) quote; short escapes for `\t\n\r`, `\xXX` for
/// other ASCII controls.
fn py_repr_string(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ if c == quote => {
                out.push('\\');
                out.push(c);
            }
            '\u{0}'..='\u{1f}' | '\u{7f}' => {
                out.push_str(&format!("\\x{:02x}", u32::from(c)));
            }
            _ => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// CPython `repr()` spelling for a float: shortest round-trip with an
/// explicit exponent sign and at least two exponent digits.
fn cpython_float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_string();
    }
    if f.is_infinite() {
        return if f.is_sign_positive() {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }
    let rendered = format!("{f:?}");
    let Some(epos) = rendered.find('e') else {
        return rendered;
    };
    let (mantissa, exponent) = rendered.split_at(epos);
    let exponent: i32 = exponent[1..].parse().unwrap_or(0);
    if exponent < 0 {
        format!("{mantissa}e-{exponent:02}", exponent = exponent.abs())
    } else {
        format!("{mantissa}e+{exponent:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static EXTRACT_FIXTURE: &str = include_str!(
        "../../../../fixtures/orchestration/fx01_types/done_signal.extract_fence.golden.json"
    );
    static PARSE_FIXTURE: &str =
        include_str!("../../../../fixtures/orchestration/fx01_types/done_signal.parse.golden.json");
    static NORMALIZE_FIXTURE: &str = include_str!(
        "../../../../fixtures/orchestration/fx01_types/done_signal.normalize.golden.json"
    );

    #[test]
    fn extract_fence_cases_match_fixture() {
        let cases: Vec<Value> = serde_json::from_str(EXTRACT_FIXTURE).expect("fixture parses");
        for case in &cases {
            let label = case["case"].as_str().unwrap();
            let input = case["input"].as_str();
            let expected = case["output"].as_str().map(str::to_string);
            assert_eq!(extract_fence(input), expected, "{label}");
        }
        assert_eq!(cases.len(), 11, "all extract cases replayed");
    }

    #[test]
    fn parse_ok_cases_match_fixture() {
        let golden: Value = serde_json::from_str(PARSE_FIXTURE).expect("fixture parses");
        assert_eq!(
            serde_json::to_value(VALID_STATUSES).unwrap(),
            golden["VALID_STATUSES"]
        );
        let cases = golden["ok"].as_array().unwrap();
        for case in cases {
            let label = case["case"].as_str().unwrap();
            let body = serde_json::to_string(&case["input_payload"]).unwrap();
            let signal = parse(&format!("```pi-dash-done\n{body}\n```")).expect(label);
            assert_eq!(
                signal.status,
                case["status"].as_str().unwrap(),
                "{label}: status"
            );
            assert_eq!(signal.payload, case["payload"], "{label}: payload");
        }
        assert_eq!(cases.len(), 4, "all ok cases replayed");
    }

    #[test]
    fn parse_error_strings_match_fixture_verbatim() {
        let golden: Value = serde_json::from_str(PARSE_FIXTURE).expect("fixture parses");
        let cases = golden["errors"].as_array().unwrap();
        for case in cases {
            let label = case["case"].as_str().unwrap();
            let err = parse(case["input"].as_str().unwrap()).expect_err(label);
            assert_eq!(err.0, case["error"].as_str().unwrap(), "{label}");
        }
        assert_eq!(cases.len(), 7, "all error cases replayed");
    }

    #[test]
    fn normalize_goldens_match_fixture() {
        let golden: Value = serde_json::from_str(NORMALIZE_FIXTURE).expect("fixture parses");
        for label in ["minimal", "rich"] {
            let case = &golden[label];
            let output = normalize(case["input"].as_object().unwrap());
            assert_eq!(output, case["output"], "{label}");
        }
    }

    // Below: oracle-verified regression pins (CPython 3.12 `json` + `re`,
    // live Django tree, 2026-10-03) beyond the fixture goldens.

    #[test]
    fn json_error_taxonomy_matches_cpython() {
        let cases: &[(&str, &str)] = &[
            ("", "Expecting value"),
            ("[", "Expecting value"),
            ("[1,", "Expecting value"),
            ("[1,]", "Expecting value"),
            ("tru", "Expecting value"),
            ("-", "Expecting value"),
            ("{", "Expecting property name enclosed in double quotes"),
            (
                "{\"a\":1,}",
                "Expecting property name enclosed in double quotes",
            ),
            ("[01]", "Expecting ',' delimiter"),
            ("{\"a\":1 2}", "Expecting ',' delimiter"),
            ("[1.]", "Expecting ',' delimiter"),
            ("{\"a\"}", "Expecting ':' delimiter"),
            ("{\"a\" 1}", "Expecting ':' delimiter"),
            ("01", "Extra data"),
            ("1.", "Extra data"),
            ("[1] [2]", "Extra data"),
            ("\"abc", "Unterminated string starting at"),
            ("\"\\x\"", "Invalid \\escape"),
            ("\"\\u12\"", "Invalid \\uXXXX escape"),
            ("\"a\x01b\"", "Invalid control character at"),
        ];
        for (body, msg) in cases {
            let err = parse(&format!("```pi-dash-done\n{body}\n```")).expect_err(body);
            assert_eq!(
                err.0,
                format!("pi-dash-done JSON invalid: {msg}"),
                "{body:?}"
            );
        }
    }

    #[test]
    fn status_repr_matches_python() {
        let cases: &[(&str, &str)] = &[
            ("4.5", "4.5"),
            ("1.0", "1.0"),
            ("1e16", "1e+16"),
            ("1.5e-05", "1.5e-05"),
            ("0.000001", "1e-06"),
            ("-0.0", "-0.0"),
            ("\"it's\"", "\"it's\""),
            ("\"it's \\\"both\\\"\"", "'it\\'s \"both\"'"),
            ("\"say \\\"hi\\\"\"", "'say \"hi\"'"),
            ("[1, \"a\"]", "[1, 'a']"),
            ("{\"k\": \"v\"}", "{'k': 'v'}"),
        ];
        for (status_json, want) in cases {
            let err = parse(&format!(
                "```pi-dash-done\n{{\"status\": {status_json}}}\n```"
            ))
            .expect_err(status_json);
            assert_eq!(
                err.0,
                format!(
                    "pi-dash-done.status must be one of \
                     ['blocked', 'completed', 'noop', 'paused']; got {want}"
                ),
                "{status_json}"
            );
        }
    }

    #[test]
    fn gap_scalars_reach_the_non_object_error() {
        // CPython parses these, then rejects them as non-objects.
        for body in ["NaN", "Infinity", "-Infinity", "\"\\ud800\""] {
            let err = parse(&format!("```pi-dash-done\n{body}\n```")).expect_err(body);
            assert_eq!(
                err.0, "pi-dash-done payload must be a JSON object",
                "{body:?}"
            );
        }
    }

    #[test]
    fn fence_edges_match_python_re() {
        // CRLF, indented, and over-long fences never open; FS chars strip.
        assert_eq!(extract_fence(Some("```pi-dash-done\r\n{}\r\n```")), None);
        assert_eq!(extract_fence(Some("  ```pi-dash-done\n{}\n```")), None);
        assert_eq!(extract_fence(Some("````pi-dash-done\n{}\n```")), None);
        assert_eq!(
            extract_fence(Some("```pi-dash-done\n\x1c{\"x\":1}\x1c\n```")),
            Some("{\"x\":1}".to_string())
        );
    }

    #[test]
    fn malformed_sections_totalize_to_defaults() {
        // Python crashes with AttributeError/TypeError here; the port
        // degrades to defaults (documented in the module notes).
        for payload in [
            serde_json::json!({"status": "completed", "autonomy": [1]}),
            serde_json::json!({"status": "completed", "autonomy": "x"}),
        ] {
            let out = normalize(payload.as_object().unwrap());
            assert_eq!(out["autonomy"]["score"], serde_json::json!(0));
            assert_eq!(out["autonomy"]["question_for_human"], Value::Null);
        }
        for status in [serde_json::json!([1, "a"]), serde_json::json!({"k": "v"})] {
            let err = parse(&format!(
                "```pi-dash-done\n{}\n```",
                serde_json::json!({"status": status})
            ))
            .expect_err("unhashable status");
            assert!(
                err.0.starts_with("pi-dash-done.status must be one of"),
                "{err}"
            );
        }
    }
}
