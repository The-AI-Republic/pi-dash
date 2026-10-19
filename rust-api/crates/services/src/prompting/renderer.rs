//! Sandboxed Jinja renderer.
//!
//! Port of `apps/api/pi_dash/prompting/renderer.py` (63 lines):
//!
//! * `renderer.py:21-31` (`PromptRenderError` / `PromptSyntaxError`) —
//!   `PromptSyntaxError` converts into `PromptRenderError`, mirroring the
//!   Python subclassing.
//! * `renderer.py:34-39` (the `SandboxedEnvironment`: no autoescape, no
//!   block trimming, `StrictUndefined`) — here a `minijinja` environment
//!   with strict undefined behavior and no autoescape (Porting guide:
//!   "`minijinja` with strict undefined"). `minijinja` is sandboxed by
//!   construction: no filesystem loader (everything renders `from_string`),
//!   no Python attribute traversal.
//! * `renderer.py:42-52` (`render`) and `55-63` (`validate_syntax`).
//!
//! Engine-message caveat: successful renders are byte-identical across
//! engines; failure *messages* are engine-specific prose, so tests assert
//! exact text for successes and the error kind for failures.
//!
//! No ported bugs: the `bugs` array of `FIX-render` is empty.

use std::sync::LazyLock;

/// Raised when a prompt template fails to render (`renderer.py:21-22`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct PromptRenderError {
    message: String,
}

impl PromptRenderError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The engine's failure message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Raised when a prompt template has invalid Jinja syntax
/// (`renderer.py:25-31`). Distinct from [`PromptRenderError`] so callers
/// that only care about syntax (e.g. save-time validation) can ignore
/// runtime issues like missing context variables; converts into
/// [`PromptRenderError`] like the Python subclass.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct PromptSyntaxError {
    message: String,
}

impl PromptSyntaxError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The engine's syntax message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<PromptSyntaxError> for PromptRenderError {
    fn from(err: PromptSyntaxError) -> Self {
        Self {
            message: err.message,
        }
    }
}

fn environment() -> minijinja::Environment<'static> {
    let mut env = minijinja::Environment::new();
    // `autoescape=False, trim_blocks=False, lstrip_blocks=False,
    // undefined=StrictUndefined` (`renderer.py:34-39`).
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
    env.set_trim_blocks(false);
    env.set_lstrip_blocks(false);
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    // Jinja2 prints interpolated values with Python `str()`; minijinja's
    // default prints maps/sequences with Rust `Debug`. The custom
    // formatter closes that gap (`{{ limits }}` renders the dict repr).
    env.set_formatter(python_formatter);
    env
}

/// Field name serde_json uses when a `Number` is serialized with the
/// `arbitrary_precision` feature (`serialize_struct(TOKEN, 1)` carrying
/// the exact decimal text). minijinja takes structs as maps, so without
/// intervention every JSON number arrives as a one-entry map under this
/// key. The token is serde_json-private (`pub(crate)`), hence the
/// literal; no prompt context builds it by hand.
const SERDE_JSON_NUMBER_TOKEN: &str = "$serde_json::private::Number";

/// `{{ value }}` with Python `str()` semantics (PIDASHCONV-1072).
///
/// Scalars delegate to the default formatter (minijinja already prints
/// `True`/`False`/`None`/ints like CPython); maps and sequences render
/// as Python `repr` — `{'k': v}` / `[v]` with single-quoted strings —
/// so `{{ limits }}` matches Jinja2 byte for byte.
fn python_formatter(
    out: &mut minijinja::Output,
    state: &minijinja::State,
    value: &minijinja::value::Value,
) -> Result<(), minijinja::Error> {
    write_python_value(out, state, value, false)
}

/// Shared `str()`/`repr()` writer: a top-level string stays bare
/// (`repr_strings = false`) while strings nested in a map or sequence
/// quote (`repr_strings = true`), exactly like CPython. Single-entry
/// [`SERDE_JSON_NUMBER_TOKEN`] maps unwrap to their digits at any
/// level, restoring numbers the `arbitrary_precision` build turns into
/// maps during context conversion.
fn write_python_value(
    out: &mut minijinja::Output,
    state: &minijinja::State,
    value: &minijinja::value::Value,
    repr_strings: bool,
) -> Result<(), minijinja::Error> {
    use minijinja::value::ValueKind;
    use std::fmt::Write as _;
    if let Some(digits) = serde_number_text(value) {
        return out.write_str(&digits).map_err(minijinja::Error::from);
    }
    match value.kind() {
        ValueKind::Map => {
            out.write_char('{').map_err(minijinja::Error::from)?;
            let mut first = true;
            for key in value.try_iter()? {
                if !first {
                    out.write_str(", ").map_err(minijinja::Error::from)?;
                }
                first = false;
                write_python_value(out, state, &key, true)?;
                out.write_str(": ").map_err(minijinja::Error::from)?;
                write_python_value(out, state, &value.get_item(&key)?, true)?;
            }
            out.write_char('}').map_err(minijinja::Error::from)
        }
        ValueKind::Seq => {
            out.write_char('[').map_err(minijinja::Error::from)?;
            let mut first = true;
            for item in value.try_iter()? {
                if !first {
                    out.write_str(", ").map_err(minijinja::Error::from)?;
                }
                first = false;
                write_python_value(out, state, &item, true)?;
            }
            out.write_char(']').map_err(minijinja::Error::from)
        }
        ValueKind::String if repr_strings => {
            let text = value.as_str().unwrap_or_default();
            out.write_str(&py_quote(text))
                .map_err(minijinja::Error::from)
        }
        _ => minijinja::escape_formatter(out, state, value),
    }
}

/// The digits of a serde_json number that crossed into a one-entry map
/// (`arbitrary_precision`), else `None`. Only the exact single-key,
/// string-valued shape unwraps; anything else formats as a map.
fn serde_number_text(value: &minijinja::value::Value) -> Option<String> {
    use minijinja::value::{Value, ValueKind};
    if value.kind() != ValueKind::Map || value.len() != Some(1) {
        return None;
    }
    let key = Value::from(SERDE_JSON_NUMBER_TOKEN);
    let digits = value.get_item(&key).ok()?;
    digits.as_str().map(str::to_owned)
}

/// Python `repr` of a string: single-quoted with backslash escapes.
/// Mirrors `jobs::pyformat::py_str`, which services cannot import (the
/// crate edge runs the other way).
fn py_quote(s: &str) -> String {
    let mut quoted = String::with_capacity(s.len() + 2);
    quoted.push('\'');
    for c in s.chars() {
        match c {
            '\\' => quoted.push_str("\\\\"),
            '\'' => quoted.push_str("\\'"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                quoted.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => quoted.push(c),
        }
    }
    quoted.push('\'');
    quoted
}

static ENV: LazyLock<minijinja::Environment<'static>> = LazyLock::new(environment);

/// Render `body` with `context` and return the resulting string
/// (`renderer.py:42-52`). Any engine error becomes a [`PromptRenderError`]
/// so call sites can fail cleanly instead of bubbling a 500.
pub fn render(body: &str, context: &serde_json::Value) -> Result<String, PromptRenderError> {
    // Convert by hand (see `mini_value`): rendering the JSON value
    // directly would serialize numbers through serde.
    let converted = mini_value(context);
    ENV.render_str(body, &converted)
        .map_err(|err| PromptRenderError::new(err.to_string()))
}

/// Convert one JSON context value into a minijinja value by hand, never
/// through serde.
///
/// Rendering `&serde_json::Value` serializes the context via serde — and
/// the workspace unifies `serde_json/arbitrary_precision` (from
/// `pidash-api`), under which `Number::serialize` emits the internal
/// token struct instead of a u64/i64/f64. minijinja then sees every JSON
/// number as a one-entry `$serde_json::private::Number` map: `{{ n }}`
/// prints the map, a `0` is truthy, and `n != 1` never holds
/// (PIDASHCONV-1073; the `tasks_mail` precedent is PIDASHCONV-865 D1).
/// Walking the value here keeps numbers numeric however the crate graph
/// unifies; rendering takes the converted value, whose own `Serialize`
/// round-trips through minijinja's value-handle fast path.
fn mini_value(value: &serde_json::Value) -> minijinja::value::Value {
    use minijinja::value::Value as MiniValue;
    use serde_json::Value as Json;
    match value {
        Json::Null => MiniValue::from(()),
        Json::Bool(flag) => MiniValue::from(*flag),
        Json::Number(number) => mini_number(number),
        Json::String(text) => MiniValue::from(text.clone()),
        Json::Array(items) => items.iter().map(mini_value).collect::<MiniValue>(),
        Json::Object(map) => map
            .iter()
            .map(|(key, item)| (key.clone(), mini_value(item)))
            .collect::<MiniValue>(),
    }
}

/// A JSON number as the narrowest minijinja number. `as_u64`/`as_i64`
/// parse the stored token, so they stay exact under
/// `arbitrary_precision`; only an out-of-range integer falls through to
/// the 128-bit parse and then to its decimal spelling (unreachable in
/// prompt contexts — counts and small ids — but must not panic; as a
/// display string it still renders the same digits).
fn mini_number(number: &serde_json::Number) -> minijinja::value::Value {
    use minijinja::value::Value as MiniValue;
    if let Some(int) = number.as_u64() {
        MiniValue::from(int)
    } else if let Some(int) = number.as_i64() {
        MiniValue::from(int)
    } else if number.is_f64() {
        // Checked above, so the parse cannot fail; `is_f64` (not
        // `as_f64().is_some()`) keeps out-of-range integers on the exact
        // 128-bit path instead of a lossy float.
        MiniValue::from(number.as_f64().expect("is_f64 checked"))
    } else if let Ok(int) = number.to_string().parse::<u128>() {
        MiniValue::from(int)
    } else if let Ok(int) = number.to_string().parse::<i128>() {
        MiniValue::from(int)
    } else {
        MiniValue::from(number.to_string())
    }
}

/// Raise [`PromptSyntaxError`] iff `body` is not a valid Jinja template
/// (`renderer.py:55-63`). Does not execute the template or require any
/// context — useful for save-time validation where missing runtime
/// variables are expected.
pub fn validate_syntax(body: &str) -> Result<(), PromptSyntaxError> {
    match ENV.template_from_str(body) {
        Ok(_) => Ok(()),
        Err(err) => Err(PromptSyntaxError::new(err.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/prompting/FIX-render.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn renderer_data(fixture: &Value) -> &Value {
        fixture
            .get("data")
            .and_then(|data| data.get("renderer"))
            .expect("fixture carries data.renderer")
    }

    #[test]
    fn env_matches_python_configuration() {
        let golden = fixture();
        let env = renderer_data(&golden)["env"].clone();
        assert_eq!(env["autoescape"], Value::Bool(false));
        assert_eq!(env["trim_blocks"], Value::Bool(false));
        assert_eq!(env["lstrip_blocks"], Value::Bool(false));
        assert_eq!(
            env["undefined"],
            Value::String("StrictUndefined".to_owned())
        );
        assert_eq!(env["loader"], Value::String("from_string only".to_owned()));
    }

    #[test]
    fn syntax_error_is_render_error() {
        // `PromptSyntaxError` subclasses `PromptRenderError`
        // (`FIX-render: error_classes`), mirrored by the `From` impl.
        let golden = fixture();
        assert!(
            renderer_data(&golden)["error_classes"]["PromptSyntaxError_is_PromptRenderError"]
                .as_bool()
                .expect("bool")
        );
        let syntax = PromptSyntaxError::new("bad syntax");
        let render: PromptRenderError = syntax.into();
        assert_eq!(render.message(), "bad syntax");
    }

    /// Rebuild each fixture render case's template from its recorded
    /// output contract and check this engine agrees on success vs failure.
    /// Templates (kept next to the test, since the fixture records the
    /// contract, not the source):
    ///
    /// * var: `Hello {{ name }}!` with `{"name": "Ada"}`
    /// * for_loop: `{% for x in items %}{{ x }};{% endfor %}`
    /// * filter_join: `{{ items|join(", ") }}`
    /// * if_true: `{% if flag %}yes{% endif %}`
    /// * if_missing_var: `{% if missing %}yes{% endif %}` (strict undefined)
    /// * sandbox_blocked: `{{ "".__class__ }}` (attribute traversal)
    #[test]
    fn render_cases_match_fixture() {
        let golden = fixture();
        let renders = renderer_data(&golden)["renders"].clone();
        let cases: &[(&str, &str, Value, bool)] = &[
            ("var", "Hello {{ name }}!", json!({"name": "Ada"}), true),
            (
                "for_loop",
                "{% for x in items %}{{ x }};{% endfor %}",
                json!({"items": ["a", "b"]}),
                true,
            ),
            (
                "filter_join",
                "{{ items|join(\", \") }}",
                json!({"items": ["a", "b"]}),
                true,
            ),
            (
                "if_true",
                "{% if flag %}yes{% endif %}",
                json!({"flag": true}),
                true,
            ),
            (
                "if_missing_var",
                "{% if missing %}yes{% endif %}",
                json!({}),
                false,
            ),
            ("sandbox_blocked", "{{ \"\".__class__ }}", json!({}), false),
        ];
        for (name, template, context, ok) in cases {
            let golden = &renders[*name];
            assert_eq!(
                golden["ok"].as_bool().expect("ok is a bool"),
                *ok,
                "fixture agrees case {name} succeeds={ok}"
            );
            match render(template, context) {
                Ok(text) => {
                    assert!(ok, "case {name} unexpectedly succeeds");
                    assert_eq!(
                        text,
                        golden["text"].as_str().expect("success carries text"),
                        "byte-identical output for {name}"
                    );
                }
                Err(err) => {
                    assert!(!ok, "case {name} unexpectedly fails: {err}");
                    assert!(
                        golden.get("error").is_some(),
                        "fixture records an error for {name}"
                    );
                    assert!(!err.message().is_empty());
                }
            }
        }
    }

    #[test]
    fn strict_undefined_applies_to_bare_variables() {
        // `{{ missing }}` with no context must fail, not render empty.
        let err = render("Hello {{ missing }}!", &json!({})).expect_err("strict undefined");
        assert!(!err.message().is_empty());
    }

    #[test]
    fn validate_syntax_accepts_valid_templates() {
        let golden = fixture();
        let syntax = renderer_data(&golden)["syntax"].clone();
        for name in ["ok_simple", "ok_if"] {
            assert!(
                syntax[name]["ok"].as_bool().expect("ok is a bool"),
                "fixture marks {name} valid"
            );
        }
        validate_syntax("Hello {{ name }}!").expect("simple template parses");
        validate_syntax("{% if flag %}yes{% endif %}").expect("if template parses");
        // Missing runtime variables are expected at save time: parse ok.
        validate_syntax("{% if missing %}yes{% endif %}").expect("undefined var parses");
    }

    /// Dual-engine agreement over every stored section body (Porting
    /// guide: "verified by rendering every stored template through both
    /// engines"). Under an empty context the Jinja2 sandbox (3.1.6) renders
    /// exactly the sections in [`EMPTY_CONTEXT_CLEAN`] and fails the rest
    /// with `UndefinedError` (verified 2026-09-28; byte-identical text on
    /// all 13 successes). This test locks the minijinja half; the Jinja2
    /// half lives in
    /// `rust-api/contract-tests/prompting/test_data_plane_dual_engine.py`.
    /// Update both sides together when a section body changes.
    const EMPTY_CONTEXT_CLEAN: &[&str] = &[
        "autonomy",
        "cloud-direct-task",
        "cloud-ending",
        "cloud-execution-loop",
        "cloud-review-loop",
        "cloud-scheduler-intro",
        "cloud-scheduler-loop",
        "cloud-test-loop",
        "default-posture",
        "guardrails",
        "review-cycle",
        "scheduler-ending",
        "test-cycle",
    ];

    #[test]
    fn all_stored_sections_parse() {
        // Shipped defaults must parse in both engines.
        for section in crate::prompting::registry::all_sections() {
            validate_syntax(&section.default_body)
                .unwrap_or_else(|err| panic!("section {} parses: {err}", section.key));
        }
    }

    #[test]
    fn all_stored_sections_render_like_jinja2() {
        let empty = serde_json::Value::Object(serde_json::Map::new());
        let mut clean: Vec<&str> = Vec::new();
        for section in crate::prompting::registry::all_sections() {
            if render(&section.default_body, &empty).is_ok() {
                clean.push(section.key);
            }
        }
        assert_eq!(clean, EMPTY_CONTEXT_CLEAN);
    }

    /// Budget-block source: lines from `### Budget` to the end of the
    /// embedded task-lifecycle body (self-contained: the `{% if tick %}`
    /// opened on the runs line closes on the last line).
    fn budget_block() -> String {
        let section = crate::prompting::registry::all_sections()
            .into_iter()
            .find(|s| s.key == "task-lifecycle")
            .expect("task-lifecycle section exists");
        let lines: Vec<&str> = section.default_body.lines().collect();
        let start = lines
            .iter()
            .position(|line| *line == "### Budget")
            .expect("Budget head exists");
        lines[start..].join("\n")
    }

    /// A `tick_context`-shaped context with the waited/allowance arms set.
    fn tick_case(waited: i64, allowance: Value) -> Value {
        json!({
            "tick": {
                "count": 3,
                "cap": 20,
                "remaining": 17,
                "waited": waited,
                "wait_allowance": allowance,
                "spent": false,
                "clock_live": true,
                "interval_seconds": 3600,
                "interval_human": "1 hour",
            },
            "issue": {"identifier": "TP-12"},
        })
    }

    /// The "Runs used on this issue" sentence renders byte-identical to
    /// Jinja2 for waited = 0/1/2 (PIDASHCONV-1073): JSON numbers must
    /// reach minijinja as numbers, so `{% if tick.waited | default(0) %}`
    /// treats 0 as falsy and `tick.waited | default(0) != 1` compares
    /// numerically. The Jinja2 half asserting the same literals lives in
    /// `rust-api/contract-tests/prompting/test_data_plane_dual_engine.py`
    /// (`test_tick_sentence_waited_counts`); update both sides together
    /// when the section body changes.
    #[test]
    fn tick_sentence_matches_jinja2_for_waited_counts() {
        let block = budget_block();
        let stem = "Runs used on this issue: **3 of 20** (17 remaining). This is one pool for the life of the issue, spent in any stage by runs the ticking system starts (timer ticks, and the run that follows a state move you make). Runs a human starts are free.";
        let tail = "each of those raised the cap by one, so they are counted above and cost no net budget.";
        let cases: &[(i64, String)] = &[
            (0, stem.to_owned()),
            (
                1,
                format!("{stem} You have ended 1 run by waiting on a blocker; {tail}"),
            ),
            (
                2,
                format!("{stem} You have ended 2 runs by waiting on a blocker; {tail}"),
            ),
        ];
        for (waited, want) in cases {
            let out = render(&block, &tick_case(*waited, Value::Null)).expect("renders");
            let line = out.lines().nth(2).expect("runs sentence is line 3");
            assert_eq!(
                line, want,
                "byte-identical tick sentence for waited={waited}"
            );
        }
    }

    /// Same defect class on the allowance arm: `tick.wait_allowance == 0`
    /// must hold for a present 0 (PIDASHCONV-1073). Jinja2 half is
    /// `test_tick_sentence_wait_allowance` in the dual-engine suite.
    #[test]
    fn wait_allowance_arm_matches_jinja2() {
        let block = budget_block();
        let stem = "When this issue has open blockers, they are information, not a gate — read them and decide for yourself whether the work can proceed (see \"Work item relationships\"). If it can, proceed and record the assumption. If it cannot, write down which blocker and why, call `pidash issue wait TP-12`, and yield `waiting_on_external`: the wait buys back this run's tick, so it costs no budget, and the next tick asks you again. Nothing watches the blocker for you — a blocker closing starts no run, and no one reads your workpad — so each tick you re-read and re-decide.";
        let tail = " While the issue stays in its current state, Pi Dash re-invokes the agent about every 1 hour.";
        let spent_clause = " **Your wait allowance is spent**: further waiting comes out of the pool above, so if you are still blocked, tell the human in a comment instead of waiting again.";
        let cases: &[(i64, String)] = &[
            (0, format!("{stem}{spent_clause}{tail}")),
            (2, format!("{stem}{tail}")),
        ];
        for (allowance, want) in cases {
            let out = render(&block, &tick_case(0, json!(*allowance))).expect("renders");
            let line = out.lines().nth(4).expect("blockers paragraph is line 5");
            assert_eq!(
                line, want,
                "byte-identical allowance arm for wait_allowance={allowance}"
            );
        }
    }

    #[test]
    fn validate_syntax_rejects_invalid_templates() {
        let golden = fixture();
        let syntax = renderer_data(&golden)["syntax"].clone();
        for name in ["bad_tag", "bad_unclosed"] {
            assert!(
                !syntax[name]["ok"].as_bool().expect("ok is a bool"),
                "fixture marks {name} invalid"
            );
            assert!(
                syntax[name].get("error").is_some(),
                "fixture records an error for {name}"
            );
        }
        let err = validate_syntax("{% if %}").expect_err("bad tag raises");
        assert!(!err.message().is_empty());
        let err = validate_syntax("Hello {{!}}").expect_err("unclosed raises");
        assert!(!err.message().is_empty());
        // Syntax failures surface as PromptSyntaxError, convertible to
        // PromptRenderError like the Python subclass.
        let render_err: PromptRenderError = validate_syntax("{% if %}").expect_err("raises").into();
        assert!(!render_err.message().is_empty());
    }

    /// Django-default limits in `policy.py:140-148` insertion order.
    fn django_default_limits() -> Value {
        json!({
            "model_requests": 25,
            "tool_calls": 20,
            "writes": 3,
            "input_tokens": 144_000,
            "output_tokens": 16_000,
            "total_tokens": 160_000,
            "wall_seconds": 285,
        })
    }

    /// `Limits: {{ limits }}.` renders the Python dict repr — exact key
    /// order, single quotes, plain ints (PIDASHCONV-1072). The Jinja2
    /// half of this agreement lives in
    /// `rust-api/contract-tests/prompting/test_data_plane_dual_engine.py`;
    /// both pin the same bytes.
    #[test]
    fn limits_line_renders_as_python_dict_repr() {
        let text = render(
            "Limits: {{ limits }}.",
            &json!({ "limits": django_default_limits() }),
        )
        .expect("renders");
        assert_eq!(
            text,
            "Limits: {'model_requests': 25, 'tool_calls': 20, 'writes': 3, \
             'input_tokens': 144000, 'output_tokens': 16000, \
             'total_tokens': 160000, 'wall_seconds': 285}."
        );
    }

    /// The whole cloud-capabilities section renders byte-identical to
    /// Jinja2 with a realistic context (PIDASHCONV-1072). Same expected
    /// text as the dual-engine contract test pins on the Python side.
    #[test]
    fn cloud_capabilities_section_renders_like_jinja2() {
        let section =
            crate::prompting::registry::get_section("cloud-capabilities").expect("section exists");
        let text = render(
            &section.default_body,
            &json!({
                "available_tools": ["pidash_get_current_issue"],
                "unavailable_capabilities": ["filesystem", "shell", "worktree"],
                "extra_toolsets": false,
                "limits": django_default_limits(),
            }),
        )
        .expect("renders");
        assert_eq!(
            text,
            "## Capabilities\n\
             \n\
             Available tools:\n\
             - `pidash_get_current_issue`\n\
             \n\
             Unavailable capabilities: filesystem, shell, worktree.\n\
             Limits: {'model_requests': 25, 'tool_calls': 20, 'writes': 3, \
             'input_tokens': 144000, 'output_tokens': 16000, \
             'total_tokens': 160000, 'wall_seconds': 285}.\n\
             \n\
             You do not have a local filesystem, shell, worktree, repository \
             checkout, or Pi Dash CLI. Do not invent or simulate them.\n"
        );
    }

    /// Numbers from a JSON context render as digits, not as the
    /// one-entry `$serde_json::private::Number` map the
    /// `arbitrary_precision` build converts them into (PIDASHCONV-1072).
    /// Covers the `{{ limits.writes | default(0) }}` interpolation in
    /// `cloud-write-policy.md` and the scalar legs (`tick.count`, …).
    #[test]
    fn scalar_numbers_render_as_digits() {
        let ctx = json!({ "limits": django_default_limits(), "x": 5 });
        assert_eq!(
            render("{{ limits.writes | default(0) }}", &ctx).expect("renders"),
            "3"
        );
        assert_eq!(render("{{ x }}", &ctx).expect("renders"), "5");
        assert_eq!(
            render("{{ missing | default(0) }}", &ctx).expect("renders"),
            "0"
        );
        assert_eq!(
            render("{{ limits.missing | default(0) }}", &ctx).expect("renders"),
            "0"
        );
    }

    /// Maps and sequences otherwise render as Python `repr`: empty
    /// containers stay bare, nested strings quote, `None`/`True` spell
    /// CPython's way, and quotes/backslashes escape (PIDASHCONV-1072).
    #[test]
    fn collections_render_as_python_repr() {
        let ctx = json!({
            "empty_map": {},
            "empty_list": [],
            "nested": {"a": [1, "x", null, true]},
            "quote": {"q": "a'b\\c"},
        });
        assert_eq!(render("{{ empty_map }}", &ctx).expect("renders"), "{}");
        assert_eq!(render("{{ empty_list }}", &ctx).expect("renders"), "[]");
        assert_eq!(
            render("{{ nested }}", &ctx).expect("renders"),
            "{'a': [1, 'x', None, True]}"
        );
        assert_eq!(
            render("{{ quote }}", &ctx).expect("renders"),
            "{'q': 'a\\'b\\\\c'}"
        );
        // A top-level string stays bare while the same string nested in a
        // container quotes, exactly like `str()` vs `repr()`.
        assert_eq!(render("{{ nested.a[1] }}!", &ctx).expect("renders"), "x!");
    }
}
