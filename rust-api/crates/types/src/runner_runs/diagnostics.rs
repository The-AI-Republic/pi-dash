//! Human-facing diagnostics for terminal agent runs (D-15, stage 5).
//!
//! Port of `apps/api/pi_dash/runner/diagnostics.py:1-245`:
//!
//! * `infer_agent_label` (`:103-128`) → [`infer_agent_label`].
//! * `enrich_run_error` (`:149-170`) → [`enrich_run_error`].
//! * `classify_run_error` (`:173-245`) → [`classify_run_error`].
//! * `AgentRunSerializer.get_error_diagnostic`
//!   (`serializers.py:266-272`, the list-serialization `None` contract) →
//!   [`error_diagnostic`]: the caller passes the `is_list` flag.
//!
//! Translation notes:
//!
//! * Python takes the Django `Runner` row and reads attributes off it; the
//!   port takes [`RunnerInfo`], a plain borrow of the same four signals
//!   (`name`, `host_label`, `capabilities`, dev-machine label/host label).
//!   `None` and `""` collapse identically on both sides; non-string
//!   attribute values are unrepresentable here (real rows hold strings).
//! * `error`/`model` are `Option<&str>`: `None` behaves as Python's
//!   `(value or "")`. Truthy non-string inputs would raise `AttributeError`
//!   in Python (`.strip()` on a list); they cannot be expressed here.
//! * `str.strip()` is Rust's `is_whitespace` set plus U+001C..=U+001F
//!   (verified over the BMP: the only delta); `str.splitlines()` splits
//!   on `\r`, `\x0b`, `\x0c` and the other non-`\n` boundaries too, so
//!   [`split_lines`] replicates the full boundary set instead of using
//!   `str::lines`.
//! * The enriched-label recovery slices the original line by the
//!   lowercased match index, exactly as Python does — including the
//!   off-by-expansion result when a pre-marker char lowercases to more
//!   than one char (verified: `"İ auth x"` recovers `"İ"` on both).
//! * `\b` in the `grok`/`muse`/`401|403` patterns is Unicode-aware on both
//!   sides (`regex` crate default, like Python `re` on `str`).
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-01-types-pure.golden.json`
//! (FX-RUN-01 `diagnostics`).
//!
//! Ported bugs: none found in this unit on read-through.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// Marker separating guidance from the raw error (`diagnostics.py:17`).
const ENRICHED_RAW_ERROR_MARKER: &str = "Raw agent error:";
/// Synthetic header suffix for enriched auth failures (`diagnostics.py:18`).
const AUTH_HEADER_SUFFIX: &str = "authentication_failed";

fn auth_status_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"\b(401|403)\b").expect("auth status pattern compiles"))
}

fn grok_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"\bgrok\b").expect("grok pattern compiles"))
}

fn muse_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"\bmuse\b").expect("muse pattern compiles"))
}

/// Python `str.strip()`: Rust's `is_whitespace` set plus U+001C..=U+001F.
fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// Python `str.splitlines()` boundaries (`\r\n` counts as one).
fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    let mut iter = text.char_indices().peekable();
    while let Some((i, ch)) = iter.next() {
        let boundary_len = match ch {
            '\n' => 1,
            '\r' => {
                if iter.peek().map(|(_, c)| *c) == Some('\n') {
                    iter.next();
                    2
                } else {
                    1
                }
            }
            '\u{b}' | '\u{c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}'
            | '\u{2029}' => ch.len_utf8(),
            _ => continue,
        };
        lines.push(&text[start..i]);
        start = i + boundary_len;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// First non-empty (stripped) line (`diagnostics.py:36-41`).
fn first_non_empty_line(value: &str) -> String {
    for line in split_lines(value) {
        let line = py_strip(line);
        if !line.is_empty() {
            return line.to_string();
        }
    }
    String::new()
}

/// Deep text walk over a capabilities value (`diagnostics.py:44-55`):
/// strings yield themselves, mappings yield keys then values in order,
/// sequences yield their items, everything else yields nothing.
fn iter_text_values<'v>(value: &'v serde_json::Value, out: &mut Vec<&'v str>) {
    match value {
        serde_json::Value::String(s) => out.push(s),
        serde_json::Value::Object(map) => {
            for (key, item) in map {
                out.push(key);
                iter_text_values(item, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                iter_text_values(item, out);
            }
        }
        _ => {}
    }
}

/// Map one text signal to an agent display name, `""` when it says nothing
/// (`diagnostics.py:58-87`).
fn match_agent_label(value: &str, from_model: bool) -> &'static str {
    let lowered = value.to_lowercase();
    if lowered.contains("claude_code")
        || lowered.contains("claude-code")
        || lowered.contains("claude code")
        || lowered.contains("claude")
    {
        return "Claude Code";
    }
    if lowered.contains("codex") {
        return "Codex";
    }
    if lowered.contains("cursor_agent")
        || lowered.contains("cursor-agent")
        || lowered.contains("cursor")
    {
        return "Cursor";
    }
    if lowered.contains("openclaw")
        || lowered.contains("open-claw")
        || lowered.contains("open_claw")
        || lowered.contains("acpx")
    {
        return "OpenClaw";
    }
    if !from_model && grok_re().is_match(&lowered) {
        return "Grok";
    }
    // Bare "muse" needs a word boundary ("museum", "amused"); the explicit
    // spellings stay substring matches ("muse_code" has no boundary).
    if lowered.contains("muse_code")
        || lowered.contains("muse-code")
        || lowered.contains("muse code")
        || muse_re().is_match(&lowered)
    {
        return "Muse Code";
    }
    ""
}

/// Recover the agent label from an enriched error's `AI agent:` line
/// (`diagnostics.py:90-100`).
fn agent_label_from_enriched_error(detail: &str) -> String {
    for line in split_lines(detail) {
        let line = py_strip(line);
        if !line.to_lowercase().starts_with("ai agent:") {
            continue;
        }
        let body = line.split_once(':').map(|(_, b)| py_strip(b)).unwrap_or("");
        let lowered = body.to_lowercase();
        if let Some(byte_idx) = lowered.find(" auth ") {
            if byte_idx > 0 {
                // Slice the original by the lowercased char count, as
                // Python's `body[:idx]` does (see module notes).
                let take = lowered[..byte_idx].chars().count();
                let prefix: String = body.chars().take(take).collect();
                return py_strip(&prefix).to_string();
            }
        }
    }
    String::new()
}

/// The runner signals `infer_agent_label`/`enrich_run_error` read: the
/// `Runner` row's `name`/`host_label`/`capabilities` plus the dev machine's
/// `label`/`host_label`. All borrows; `None` behaves as Python's
/// `(value or "")`.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunnerInfo<'a> {
    pub name: Option<&'a str>,
    pub host_label: Option<&'a str>,
    pub capabilities: Option<&'a serde_json::Value>,
    pub dev_machine: Option<DevMachineInfo<'a>>,
}

/// The dev-machine half of [`RunnerInfo`]: `label` wins over `host_label`,
/// exactly as the `or` chain in `_runner_location` resolves.
#[derive(Debug, Clone, Copy, Default)]
pub struct DevMachineInfo<'a> {
    pub label: Option<&'a str>,
    pub host_label: Option<&'a str>,
}

/// Best-effort display name for the local agent behind a run
/// (`diagnostics.py:103-128`): model, then capabilities, then name/host
/// label, with the raw error text as a last resort.
pub fn infer_agent_label(
    runner: Option<&RunnerInfo>,
    error: Option<&str>,
    model: Option<&str>,
) -> &'static str {
    let label = match_agent_label(model.unwrap_or(""), true);
    if !label.is_empty() {
        return label;
    }
    if let Some(runner) = runner {
        if let Some(capabilities) = runner.capabilities {
            let mut texts = Vec::new();
            iter_text_values(capabilities, &mut texts);
            for text in texts {
                let label = match_agent_label(text, false);
                if !label.is_empty() {
                    return label;
                }
            }
        }
        for attr in [runner.name.unwrap_or(""), runner.host_label.unwrap_or("")] {
            let label = match_agent_label(attr, false);
            if !label.is_empty() {
                return label;
            }
        }
    }
    match_agent_label(error.unwrap_or(""), false)
}

/// Where the operator must go to re-authenticate
/// (`diagnostics.py:131-146`).
fn runner_location(runner: Option<&RunnerInfo>) -> String {
    let runner_name = py_strip(runner.and_then(|r| r.name).unwrap_or(""));
    let dev_raw = match runner.and_then(|r| r.dev_machine) {
        Some(dev) => {
            let label = dev.label.unwrap_or("");
            if label.is_empty() {
                dev.host_label.unwrap_or("")
            } else {
                label
            }
        }
        None => "",
    };
    let mut machine_label = py_strip(dev_raw);
    if machine_label.is_empty() {
        machine_label = py_strip(runner.and_then(|r| r.host_label).unwrap_or(""));
    }
    if !machine_label.is_empty() && !runner_name.is_empty() {
        format!("dev machine \"{machine_label}\" for runner \"{runner_name}\"")
    } else if !runner_name.is_empty() {
        format!("dev machine for runner \"{runner_name}\"")
    } else if !machine_label.is_empty() {
        format!("dev machine \"{machine_label}\"")
    } else {
        "dev machine".to_string()
    }
}

/// Synthetic header line for an enriched auth failure
/// (`diagnostics.py:22-33`): the first `401`/`403` in the text, else bare.
fn auth_header(detail: &str) -> String {
    if let Some(caps) = auth_status_re().captures(detail) {
        format!("{} {AUTH_HEADER_SUFFIX}", &caps[1])
    } else {
        AUTH_HEADER_SUFFIX.to_string()
    }
}

/// Who failed: the spawned agent, or Pi Dash itself
/// (`classify_run_error`, `diagnostics.py:173-245`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunErrorSource {
    Agent,
    PidashCloud,
    PidashRunner,
    Unknown,
}

impl RunErrorSource {
    /// The verbatim wire value (`agent`, …).
    pub fn value(&self) -> &'static str {
        match self {
            RunErrorSource::Agent => "agent",
            RunErrorSource::PidashCloud => "pidash_cloud",
            RunErrorSource::PidashRunner => "pidash_runner",
            RunErrorSource::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for RunErrorSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// Stable category the UI renders without text matching
/// (`classify_run_error`, `diagnostics.py:173-245`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunErrorKind {
    AgentAuthentication,
    AgentModelAccess,
    RunnerRegistration,
    RunnerLifecycle,
    AgentStalled,
    Unknown,
}

impl RunErrorKind {
    /// The verbatim wire value (`agent_authentication`, …).
    pub fn value(&self) -> &'static str {
        match self {
            RunErrorKind::AgentAuthentication => "agent_authentication",
            RunErrorKind::AgentModelAccess => "agent_model_access",
            RunErrorKind::RunnerRegistration => "runner_registration",
            RunErrorKind::RunnerLifecycle => "runner_lifecycle",
            RunErrorKind::AgentStalled => "agent_stalled",
            RunErrorKind::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for RunErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// Compact diagnostic for a stored run error (`diagnostics.py:173-245`),
/// serialized in the Python dict key order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunErrorDiagnostic {
    pub source: RunErrorSource,
    pub source_label: String,
    pub kind: RunErrorKind,
    pub summary: String,
    pub action: String,
}

/// Compact diagnostic for a stored run error, or `None` when empty
/// (`diagnostics.py:173-245`).
pub fn classify_run_error(error: Option<&str>) -> Option<RunErrorDiagnostic> {
    let detail = py_strip(error.unwrap_or(""));
    if detail.is_empty() {
        return None;
    }
    let lowered = detail.to_lowercase();
    let summary = first_non_empty_line(detail);
    let agent_label = agent_label_from_enriched_error(detail);

    if lowered.contains("invalid authentication credentials")
        || lowered.contains("authentication_failed")
        || lowered.contains("failed to authenticate")
    {
        let action_agent = if agent_label.is_empty() {
            "the agent CLI"
        } else {
            agent_label.as_str()
        };
        let source_label = if agent_label.is_empty() {
            "Agent CLI"
        } else {
            agent_label.as_str()
        };
        return Some(RunErrorDiagnostic {
            source: RunErrorSource::Agent,
            source_label: source_label.to_string(),
            kind: RunErrorKind::AgentAuthentication,
            summary,
            action: format!("Re-authenticate {action_agent} on the runner machine, then restart the Pi Dash runner."),
        });
    }
    if lowered.contains("selected model")
        && (lowered.contains("may not exist") || lowered.contains("may not have access"))
    {
        return Some(RunErrorDiagnostic {
            source: RunErrorSource::Agent,
            source_label: "Agent CLI".to_string(),
            kind: RunErrorKind::AgentModelAccess,
            summary,
            action: "Choose a model the agent account can access, then retry the run.".to_string(),
        });
    }
    if lowered.contains("runner_not_found") || lowered.contains("run_not_owned_by_runner") {
        return Some(RunErrorDiagnostic {
            source: RunErrorSource::PidashCloud,
            source_label: "Pi Dash cloud".to_string(),
            kind: RunErrorKind::RunnerRegistration,
            summary,
            action: "Remove or re-add the stale local runner registration.".to_string(),
        });
    }
    if lowered.contains("daemon shutdown requested")
        || lowered.contains("runner revoked")
        || lowered.contains("session_evicted")
    {
        return Some(RunErrorDiagnostic {
            source: RunErrorSource::PidashRunner,
            source_label: "Pi Dash runner".to_string(),
            kind: RunErrorKind::RunnerLifecycle,
            summary,
            action:
                "Check runner service status and restart the runner if it should still accept work."
                    .to_string(),
        });
    }
    if lowered.contains("agent stalled") || lowered.contains("without new agent events") {
        return Some(RunErrorDiagnostic {
            source: RunErrorSource::Agent,
            source_label: "Agent CLI".to_string(),
            kind: RunErrorKind::AgentStalled,
            summary,
            action:
                "Inspect the runner machine for a stuck agent process or long-running tool call."
                    .to_string(),
        });
    }
    Some(RunErrorDiagnostic {
        source: RunErrorSource::Unknown,
        source_label: "Unknown".to_string(),
        kind: RunErrorKind::Unknown,
        summary,
        action: String::new(),
    })
}

/// The serializer's `error_diagnostic` field (`serializers.py:266-272`):
/// list views skip the classifier's per-row scans and render `None`.
pub fn error_diagnostic(error: Option<&str>, is_list: bool) -> Option<RunErrorDiagnostic> {
    if is_list {
        None
    } else {
        classify_run_error(error)
    }
}

/// Add operator guidance before persisting known agent failures
/// (`diagnostics.py:149-170`): empty and already-enriched errors pass
/// through (stripped); only `agent_authentication` is enriched.
pub fn enrich_run_error(
    error: Option<&str>,
    runner: Option<&RunnerInfo>,
    model: Option<&str>,
) -> String {
    let detail = py_strip(error.unwrap_or(""));
    if detail.is_empty() || detail.contains(ENRICHED_RAW_ERROR_MARKER) {
        return detail.to_string();
    }
    let diagnostic = classify_run_error(Some(detail));
    match diagnostic {
        Some(d) if d.kind == RunErrorKind::AgentAuthentication => {}
        _ => return detail.to_string(),
    }
    let agent_label = infer_agent_label(runner, Some(detail), model);
    let auth_subject = if agent_label.is_empty() {
        "AI agent auth".to_string()
    } else {
        format!("{agent_label} auth")
    };
    let agent_command = if agent_label.is_empty() {
        "the AI agent"
    } else {
        agent_label
    };
    format!(
        "{}\nAI agent: {auth_subject} appears expired or invalid. Go to the {} and re-authenticate {agent_command}, then restart the Pi Dash runner.\n\n{ENRICHED_RAW_ERROR_MARKER}\n{detail}",
        auth_header(detail),
        runner_location(runner),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    static FIXTURE: &str =
        include_str!("../../../../fixtures/runner_runs/fx-run-01-types-pure.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn diag_section(fx: &Value) -> &serde_json::Map<String, Value> {
        fx.get("diagnostics")
            .and_then(Value::as_object)
            .expect("diagnostics section")
    }

    fn runner<'a>(
        name: Option<&'a str>,
        host_label: Option<&'a str>,
        capabilities: Option<&'a Value>,
        dev_machine: Option<DevMachineInfo<'a>>,
    ) -> RunnerInfo<'a> {
        RunnerInfo {
            name,
            host_label,
            capabilities,
            dev_machine,
        }
    }

    #[test]
    fn infer_agent_label_replays_fixture_vectors() {
        let fx = fixture();
        let vectors = diag_section(&fx)
            .get("infer_agent_label")
            .and_then(Value::as_array)
            .expect("infer vectors");
        assert_eq!(vectors.len(), 17);
        let caps_executor = json!({"executor": "codex"});
        let caps_list = json!(["cursor_agent"]);
        let caps_nested = json!({"a": {"b": ["openclaw"]}});
        let caps_cursor = json!(["cursor"]);
        let caps_codex = json!(["codex"]);
        // (case, runner, error, model, expected-out from the fixture)
        let r_codex = runner(Some(""), Some(""), Some(&caps_executor), None);
        let r_list = runner(Some(""), Some(""), Some(&caps_list), None);
        let r_nested = runner(Some(""), Some(""), Some(&caps_nested), None);
        let r_grok = runner(Some("grok-runner-1"), Some(""), None, None);
        let r_museum = runner(Some("museum-host"), Some(""), None, None);
        let r_muse = runner(Some("muse_code daemon"), Some(""), None, None);
        let r_host = runner(Some(""), Some("My Claude box"), None, None);
        let r_plain = runner(Some("plain"), Some(""), None, None);
        let r_model_caps = runner(Some(""), Some(""), Some(&caps_cursor), None);
        let r_caps_name = runner(Some("cursor box"), Some(""), Some(&caps_codex), None);
        let r_name_host = runner(Some("codex"), Some("cursor"), None, None);
        type InferCase<'x> = (
            &'x str,
            Option<&'x RunnerInfo<'x>>,
            Option<&'x str>,
            Option<&'x str>,
        );
        let cases: &[InferCase] = &[
            ("model-claude", None, None, Some("claude-opus-4-5")),
            ("model-grok-weak", None, None, Some("grok-4.3")),
            ("model-empty", None, None, None),
            ("caps-codex", Some(&r_codex), None, None),
            ("caps-list", Some(&r_list), None, None),
            ("caps-nested", Some(&r_nested), None, None),
            ("runner-name-grok", Some(&r_grok), None, None),
            ("runner-name-museum", Some(&r_museum), None, None),
            ("runner-name-muse-code", Some(&r_muse), None, None),
            ("host-label", Some(&r_host), None, None),
            ("error-fallback", None, Some("claude crashed"), None),
            (
                "error-path-mention",
                Some(&r_plain),
                Some("/Users/claude/x failed"),
                None,
            ),
            (
                "precedence-model-over-caps",
                Some(&r_model_caps),
                None,
                Some("codex"),
            ),
            ("precedence-caps-over-name", Some(&r_caps_name), None, None),
            ("precedence-name-over-host", Some(&r_name_host), None, None),
            ("nothing", None, None, None),
            ("runner-none", None, Some(""), None),
        ];
        for (v, (case, r, err, model)) in vectors.iter().zip(cases.iter()) {
            assert_eq!(
                v.get("case").and_then(Value::as_str),
                Some(*case),
                "case order"
            );
            let expected = v.get("out").and_then(Value::as_str).expect("out");
            assert_eq!(
                infer_agent_label(*r, *err, *model),
                expected,
                "infer_agent_label({case})"
            );
        }
    }

    #[test]
    fn enrich_run_error_replays_fixture_vectors() {
        let fx = fixture();
        let vectors = diag_section(&fx)
            .get("enrich_run_error")
            .and_then(Value::as_array)
            .expect("enrich vectors");
        assert_eq!(vectors.len(), 8);
        let r_auth = runner(
            Some("r1"),
            Some(""),
            None,
            Some(DevMachineInfo {
                label: Some("mac"),
                host_label: None,
            }),
        );
        for v in vectors {
            let case = v.get("case").and_then(Value::as_str).unwrap_or("?");
            let input = v.get("in").and_then(Value::as_str).unwrap_or("");
            let expected = v.get("out").and_then(Value::as_str).expect("out");
            let actual = match case {
                "auth-with-runner" => enrich_run_error(Some(input), Some(&r_auth), None),
                "auth-with-model" => enrich_run_error(Some(input), None, Some("codex-mini")),
                _ => enrich_run_error(Some(input), None, None),
            };
            assert_eq!(actual, expected, "enrich_run_error({case})");
        }
    }

    /// Unescape a Python string literal body (`\\n` → newline, …).
    fn unescape_py(body: &str) -> String {
        let mut out = String::with_capacity(body.len());
        let mut chars = body.chars();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                out.push(ch);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('\'') => out.push('\''),
                Some('"') => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        }
        out
    }

    /// The fixture records classify inputs as Python reprs; `None`/`[]`
    /// both classify as `None` (`(value or "").strip()` is empty).
    fn classify_input(repr: &str) -> Option<String> {
        match repr {
            "None" | "[]" => None,
            s if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 => {
                Some(unescape_py(&s[1..s.len() - 1]))
            }
            other => panic!("unmapped classify repr {other}"),
        }
    }

    #[test]
    fn classify_run_error_replays_fixture_vectors_in_order() {
        let fx = fixture();
        let vectors = diag_section(&fx)
            .get("classify_run_error")
            .and_then(Value::as_array)
            .expect("classify vectors");
        assert_eq!(vectors.len(), 18);
        for v in vectors {
            let input = v.get("in").and_then(Value::as_str).expect("in repr");
            let expected = v.get("out").expect("out");
            let parsed = classify_input(input);
            let actual = classify_run_error(parsed.as_deref());
            let actual_value = actual
                .map(|d| serde_json::to_value(&d).expect("serializes"))
                .unwrap_or(Value::Null);
            assert_eq!(&actual_value, expected, "classify_run_error({input})");
            if let Some(obj) = actual_value.as_object() {
                let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
                assert_eq!(
                    keys,
                    ["source", "source_label", "kind", "summary", "action"],
                    "diagnostic key order"
                );
            }
        }
    }

    #[test]
    fn error_diagnostic_list_flag_skips_classifier() {
        let err = Some("Invalid authentication credentials for model");
        assert!(error_diagnostic(err, true).is_none());
        let detail = error_diagnostic(err, false).expect("detail classifies");
        assert_eq!(detail.kind, RunErrorKind::AgentAuthentication);
        assert!(error_diagnostic(None, false).is_none());
        assert!(error_diagnostic(err, true).is_none());
    }

    #[test]
    fn diagnostics_match_python_edges() {
        // Verified against the live Python functions before encoding.
        // splitlines boundaries in the summary (not just \n).
        assert_eq!(classify_run_error(Some("a\rb")).expect("d").summary, "a");
        assert_eq!(classify_run_error(Some("x\r\ny")).expect("d").summary, "x");
        assert_eq!(classify_run_error(Some("p\x0bq")).expect("d").summary, "p");
        // Lowercase-expansion slice recovery (İ lowercases to two chars).
        assert_eq!(agent_label_from_enriched_error("AI agent: İ auth x"), "İ");
        // Word-boundary labels.
        assert_eq!(match_agent_label("museum-host", false), "");
        assert_eq!(match_agent_label("amused box", false), "");
        assert_eq!(match_agent_label("muse", false), "Muse Code");
        assert_eq!(match_agent_label("grokking", false), "");
        assert_eq!(match_agent_label("my_grok", false), "");
        assert_eq!(match_agent_label("a grok b", false), "Grok");
        assert_eq!(match_agent_label("grok-4.3", true), "");
        // Auth header: first 401/403 wins; bare otherwise.
        assert_eq!(auth_header("error 4010 bad"), "authentication_failed");
        assert_eq!(auth_header("403 then 401"), "403 authentication_failed");
        // strip() parity incl. U+001C..=U+001F.
        assert_eq!(py_strip("\u{1c}y\u{1d}"), "y");
        assert!(classify_run_error(Some(" \u{1c} ")).is_none());
        // Runner-location branches.
        let none = runner_location(None);
        assert_eq!(none, "dev machine");
        let r = runner(Some("r1"), Some("h1"), None, None);
        assert_eq!(
            runner_location(Some(&r)),
            "dev machine \"h1\" for runner \"r1\""
        );
        let r = runner(Some("r1"), Some(""), None, None);
        assert_eq!(runner_location(Some(&r)), "dev machine for runner \"r1\"");
        let r = runner(Some(""), Some("h1"), None, None);
        assert_eq!(runner_location(Some(&r)), "dev machine \"h1\"");
        let r = runner(
            Some(""),
            Some(""),
            None,
            Some(DevMachineInfo {
                label: None,
                host_label: Some("mh"),
            }),
        );
        assert_eq!(runner_location(Some(&r)), "dev machine \"mh\"");
    }
}
