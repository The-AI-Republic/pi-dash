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
    env
}

static ENV: LazyLock<minijinja::Environment<'static>> = LazyLock::new(environment);

/// Render `body` with `context` and return the resulting string
/// (`renderer.py:42-52`). Any engine error becomes a [`PromptRenderError`]
/// so call sites can fail cleanly instead of bubbling a 500.
pub fn render(body: &str, context: &serde_json::Value) -> Result<String, PromptRenderError> {
    ENV.render_str(body, context)
        .map_err(|err| PromptRenderError::new(err.to_string()))
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
}
